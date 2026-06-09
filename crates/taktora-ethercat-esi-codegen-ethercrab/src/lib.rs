//! Code-emitting [`CodegenBackend`] targeting the `taktora-ethercat-esi-rt`
//! runtime contract.
//!
//! [`EthercrabBackend`] consumes the policy-resolved [`Device`] IR from
//! [`taktora_ethercat_esi_codegen`] and emits, per device:
//!
//! 1. a `#[derive(Debug, Default, Clone)]` struct with one field per non-padding
//!    PDO entry (`REQ_0521`),
//! 2. a `pub const <CONST>: Identity = …` (`REQ_0522`),
//! 3. an `impl EsiDevice` whose `decode_inputs` reads each input entry out of
//!    the process image and whose `encode_outputs` writes each output entry.
//!
//! This crate only produces tokens; it does not depend on `ethercrab`,
//! `bitvec`, or the rt crate. The generated code (compiled downstream) is what
//! links `taktora-ethercat-esi-rt`. Despite the crate name, the `ethercrab`
//! dependency only arrives with the later `configure()` slice.
//!
//! Naming policy lives in [`taktora_ethercat_esi_codegen`] (`REQ_0511`); this
//! backend calls [`taktora_ethercat_esi_codegen::field_ident`] and never
//! re-derives identifiers.

use proc_macro2::{Ident, TokenStream};
use quote::quote;
use taktora_ethercat_esi::Pdo;
use taktora_ethercat_esi_codegen::{
    CodegenBackend, CodegenError, Device, Identity, field_ident, op_mode_enum_ident,
    op_mode_variant_ident, op_mode_variant_struct_ident, pdo_field_ident, pdo_struct_ident,
};

mod typemap;

use typemap::{Layout, ScalarKind};

/// The code-emitting backend producing `EsiDevice` impls for the rt contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct EthercrabBackend;

/// One resolved input/output field: its struct field ident, Rust type, and the
/// bit layout used to build the read/write expression.
struct ResolvedField {
    ident: Ident,
    rust_type: TokenStream,
    layout: Layout,
    /// An optional doc-comment line for width-inferred opaque mappings, emitted
    /// on the generated struct field so the code self-documents the unmodelled
    /// `CoE` type. `None` for clean scalars.
    doc: Option<String>,
}

/// A resolved PDO: its device-struct field ident, its sub-struct type ident,
/// and the entry fields it owns. Used only in the multi-PDO (sub-struct) shape.
struct ResolvedPdo {
    field_ident: Ident,
    struct_ident: Ident,
    fields: Vec<ResolvedField>,
}

/// Resolve one PDO's non-padding entries into [`ResolvedField`]s, advancing the
/// shared running `offset` over declaration order. Padding entries (`index ==
/// 0`) advance the offset but emit no field. Intra-PDO field-name collisions
/// (two entries that snake-case to the same ident) are disambiguated with a
/// deterministic numeric suffix.
fn resolve_pdo_fields(pdo: &Pdo, offset: &mut usize) -> Result<Vec<ResolvedField>, CodegenError> {
    let mut fields = Vec::new();
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for entry in &pdo.entries {
        // Padding / gap entry: advance the running offset, emit no field.
        if entry.index == 0 {
            *offset += entry.bit_length as usize;
            continue;
        }

        let raw_name = entry.name.clone().unwrap_or_else(|| {
            // Unnamed non-padding entry: entry_<index:04x>_<sub>.
            format!("entry_{:04x}_{}", entry.index, entry.sub_index)
        });
        let ident = dedup_ident(&field_ident(&raw_name)?, &mut seen)?;

        let (field_type, layout) = typemap::resolve(
            entry.data_type.as_ref(),
            entry.bit_length,
            *offset,
            entry.index,
            entry.sub_index,
            &raw_name,
        )?;

        *offset += layout.width();
        fields.push(ResolvedField {
            ident,
            rust_type: field_type.rust_type,
            layout,
            doc: field_type.doc,
        });
    }

    Ok(fields)
}

/// Disambiguate an intra-PDO field ident: the first occurrence is used as-is;
/// each later collision gets a deterministic numeric suffix (`output`,
/// `output_2`, `output_3`, …). Cross-PDO collisions are already resolved by the
/// per-PDO sub-structs.
fn dedup_ident(
    ident: &Ident,
    seen: &mut std::collections::HashMap<String, usize>,
) -> Result<Ident, CodegenError> {
    let base = ident.to_string();
    let count = seen.entry(base.clone()).or_insert(0);
    *count += 1;
    if *count == 1 {
        Ok(ident.clone())
    } else {
        field_ident(&format!("{base}_{count}"))
    }
}

/// Resolve all PDOs in one direction, running a single bit-offset accumulator
/// across every PDO in declaration order (so decode/encode offsets span the
/// whole direction). Returns the per-PDO resolution and the total bit width.
fn resolve_direction(
    pdos: &[&Pdo],
    device_struct: &Ident,
) -> Result<(Vec<ResolvedPdo>, usize), CodegenError> {
    let mut resolved = Vec::with_capacity(pdos.len());
    let mut offset = 0usize;
    for pdo in pdos {
        let fields = resolve_pdo_fields(pdo, &mut offset)?;
        resolved.push(ResolvedPdo {
            field_ident: pdo_field_ident(pdo.name.as_deref(), pdo.index)?,
            struct_ident: pdo_struct_ident(device_struct, pdo.name.as_deref(), pdo.index)?,
            fields,
        });
    }
    Ok((resolved, offset))
}

/// One resolved assignment ("OpMode"): its enum-variant ident, its per-variant
/// data-struct ident, and the resolved input/output PDOs (already grouped into
/// the existing per-PDO `ResolvedPdo` shape, laid out from offset 0 within the
/// variant). Plus the raw Rx/Tx index lists for `pdo_assignment()`.
struct ResolvedAssignment {
    variant_ident: Ident,
    struct_ident: Ident,
    inputs: Vec<ResolvedPdo>,
    input_bits: usize,
    outputs: Vec<ResolvedPdo>,
    output_bits: usize,
    rx_indices: Vec<u16>,
    tx_indices: Vec<u16>,
}

/// Resolve a device's assignment set (`REQ_0523`, issue #70).
fn resolve_assignments(
    device_struct: &Ident,
    tx_pdos: &[Pdo],
    rx_pdos: &[Pdo],
    mappings: &[taktora_ethercat_esi::AlternativeSmMapping],
) -> Result<Vec<ResolvedAssignment>, CodegenError> {
    if mappings.is_empty() {
        return Ok(vec![resolve_default_assignment(
            device_struct,
            tx_pdos,
            rx_pdos,
        )?]);
    }
    // Default mapping first (stable), then the rest in document order.
    let mut order: Vec<usize> = (0..mappings.len()).collect();
    order.sort_by_key(|&i| !mappings[i].default); // default (true) sorts first

    // Phase 1: compute every mapping's (variant_ident, struct_ident) pair AND
    // validate/collect its Rx/Tx index lists, but DON'T resolve directions yet.
    // Both idents derive from the same name segment, so PascalCase-colliding
    // names produce identical pairs here.
    let mut planned = Vec::with_capacity(order.len());
    for (ordinal, &mi) in order.iter().enumerate() {
        let m = &mappings[mi];
        let mut rx_indices = Vec::new();
        let mut tx_indices = Vec::new();
        for sm in &m.sm_assignments {
            for p in &sm.pdos {
                if rx_pdos.iter().any(|q| q.index == p.index) {
                    rx_indices.push(p.index);
                } else if tx_pdos.iter().any(|q| q.index == p.index) {
                    tx_indices.push(p.index);
                } else {
                    return Err(CodegenError::UnknownAssignmentPdo {
                        device: device_struct.to_string(),
                        index: p.index,
                    });
                }
            }
        }
        planned.push(Planned {
            variant_ident: op_mode_variant_ident(m.name.as_deref(), ordinal)?,
            struct_ident: op_mode_variant_struct_ident(device_struct, m.name.as_deref(), ordinal)?,
            rx_indices,
            tx_indices,
        });
    }

    // Phase 2: dedup BOTH idents in lockstep, keyed on the struct ident base, so
    // the SAME numeric suffix lands on the variant ident and the struct ident.
    // This guarantees every struct_ident is unique BEFORE any direction is
    // resolved, so all derived idents (In/Out direction structs and per-PDO
    // sub-structs) pick up the suffix consistently.
    dedup_planned(&mut planned);

    // Phase 3: resolve directions from the FINAL (deduped) struct idents.
    let mut out = Vec::with_capacity(planned.len());
    for p in planned {
        let outputs_src = collect_pdos(rx_pdos, &p.rx_indices);
        let inputs_src = collect_pdos(tx_pdos, &p.tx_indices);
        let (inputs, input_bits) = resolve_direction(&inputs_src, &p.struct_ident)?;
        let (outputs, output_bits) = resolve_direction(&outputs_src, &p.struct_ident)?;
        out.push(ResolvedAssignment {
            variant_ident: p.variant_ident,
            struct_ident: p.struct_ident,
            inputs,
            input_bits,
            outputs,
            output_bits,
            rx_indices: p.rx_indices,
            tx_indices: p.tx_indices,
        });
    }
    Ok(out)
}

/// Build the single synthetic default assignment for a device with no
/// `AlternativeSmMapping`: default set is PDOs with an `Sm=` attribute or
/// `Mandatory` (issue #70: `Fixed` is orthogonal to assignment).
fn resolve_default_assignment(
    device_struct: &Ident,
    tx_pdos: &[Pdo],
    rx_pdos: &[Pdo],
) -> Result<ResolvedAssignment, CodegenError> {
    let in_default: Vec<&Pdo> = tx_pdos
        .iter()
        .filter(|p| p.sm.is_some() || p.mandatory)
        .collect();
    let out_default: Vec<&Pdo> = rx_pdos
        .iter()
        .filter(|p| p.sm.is_some() || p.mandatory)
        .collect();
    let rx_indices: Vec<u16> = out_default.iter().map(|p| p.index).collect();
    let tx_indices: Vec<u16> = in_default.iter().map(|p| p.index).collect();
    let variant_struct = op_mode_variant_struct_ident(device_struct, Some("Default"), 0)?;
    let (inputs, input_bits) = resolve_direction(&in_default, &variant_struct)?;
    let (outputs, output_bits) = resolve_direction(&out_default, &variant_struct)?;
    Ok(ResolvedAssignment {
        variant_ident: op_mode_variant_ident(Some("Default"), 0)?,
        struct_ident: variant_struct,
        inputs,
        input_bits,
        outputs,
        output_bits,
        rx_indices,
        tx_indices,
    })
}

/// Gather the `&Pdo`s for a list of indices, in index order, skipping missing.
///
/// Callers pre-validate that every index resolves to a member of `pool` (see
/// `resolve_assignments`'s `UnknownAssignmentPdo` check and
/// `resolve_default_assignment`, which derives indices directly from the pool),
/// so a miss is currently impossible; the silent skip is defensive only.
fn collect_pdos<'a>(pool: &'a [Pdo], indices: &[u16]) -> Vec<&'a Pdo> {
    indices
        .iter()
        .filter_map(|&idx| pool.iter().find(|p| p.index == idx))
        .collect()
}

/// A planned assignment's idents and index lists, computed BEFORE directions are
/// resolved so the variant/struct idents can be deduped in lockstep first.
struct Planned {
    variant_ident: Ident,
    struct_ident: Ident,
    rx_indices: Vec<u16>,
    tx_indices: Vec<u16>,
}

/// De-duplicate assignment idents that PascalCase-collide, applying the SAME
/// numeric suffix to BOTH the `variant_ident` and the `struct_ident` so each
/// stays paired and every `struct_ident` is unique. Keyed on the `struct_ident`
/// base (the device-prefixed name); since both idents share the colliding name
/// segment, one collision count drives both. Running this BEFORE
/// `resolve_direction` ensures all derived idents (the In/Out direction structs
/// and the per-PDO sub-structs built from `struct_ident`) inherit the suffix.
fn dedup_planned(planned: &mut [Planned]) {
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for p in planned.iter_mut() {
        let base = p.struct_ident.to_string();
        let count = seen.entry(base).or_insert(0);
        *count += 1;
        if *count > 1 {
            p.variant_ident = Ident::new(
                &format!("{}{count}", p.variant_ident),
                proc_macro2::Span::call_site(),
            );
            p.struct_ident = Ident::new(
                &format!("{}{count}", p.struct_ident),
                proc_macro2::Span::call_site(),
            );
        }
    }
}

/// Read `field` out of `bits` into an arbitrary `target` lvalue token (e.g.
/// `self.value` or a match binding `v.value`).
fn read_into(target: &TokenStream, field: &ResolvedField) -> TokenStream {
    match field.layout {
        Layout::Bool { offset } => quote! { #target = bits[#offset]; },
        Layout::Field {
            offset,
            width,
            kind,
        } => {
            let end = offset + width;
            let range = quote! { #offset..#end };
            match kind {
                ScalarKind::Int => {
                    let ty = &field.rust_type;
                    quote! { #target = bits[#range].load_le::<#ty>(); }
                }
                ScalarKind::F32 => {
                    quote! { #target = f32::from_bits(bits[#range].load_le::<u32>()); }
                }
                ScalarKind::F64 => {
                    quote! { #target = f64::from_bits(bits[#range].load_le::<u64>()); }
                }
            }
        }
        Layout::Bytes { offset, bytes } => {
            // Copy each byte out of the (byte-rounded) bit range. Per-byte
            // `load_le::<u8>` is alignment-agnostic, so the entry need not start
            // on a byte boundary.
            quote! {
                {
                    let mut buf = [0u8; #bytes];
                    for (i, b) in buf.iter_mut().enumerate() {
                        let start = #offset + i * 8;
                        *b = bits[start..start + 8].load_le::<u8>();
                    }
                    #target = buf;
                }
            }
        }
    }
}

/// Write an arbitrary `source` rvalue token (e.g. `self.value` or `v.value`)
/// for `field` into `bits`.
fn write_from(source: &TokenStream, field: &ResolvedField) -> TokenStream {
    match field.layout {
        Layout::Bool { offset } => quote! { bits.set(#offset, #source); },
        Layout::Field {
            offset,
            width,
            kind,
        } => {
            let end = offset + width;
            let range = quote! { #offset..#end };
            match kind {
                ScalarKind::Int => {
                    let ty = &field.rust_type;
                    quote! { bits[#range].store_le::<#ty>(#source); }
                }
                ScalarKind::F32 => {
                    quote! { bits[#range].store_le::<u32>(#source.to_bits()); }
                }
                ScalarKind::F64 => {
                    quote! { bits[#range].store_le::<u64>(#source.to_bits()); }
                }
            }
        }
        Layout::Bytes { offset, bytes } => {
            quote! {
                for i in 0..#bytes {
                    let start = #offset + i * 8;
                    bits[start..start + 8].store_le::<u8>(#source[i]);
                }
            }
        }
    }
}

impl CodegenBackend for EthercrabBackend {
    fn emit_device(&self, device: &Device) -> Result<TokenStream, CodegenError> {
        let struct_ident = &device.struct_ident;
        let const_ident = &device.const_ident;
        let enum_ident = op_mode_enum_ident(struct_ident)?;
        let assigns = resolve_assignments(
            struct_ident,
            device.tx_pdos,
            device.rx_pdos,
            device.alt_sm_mappings,
        )?;
        let variant_types: TokenStream = assigns.iter().map(emit_variant_types).collect();
        let enum_def = emit_op_mode_enum(&enum_ident, &assigns);
        let Identity {
            vendor_id,
            product_code,
            revision,
        } = device.identity;
        let input_len = emit_len(&enum_ident, &assigns, true);
        let output_len = emit_len(&enum_ident, &assigns, false);
        let decode_body = emit_decode(&enum_ident, &assigns);
        let encode_body = emit_encode(&enum_ident, &assigns);
        let pdo_assignment_body = emit_pdo_assignment(&enum_ident, &assigns);
        Ok(quote! {
            #variant_types

            #enum_def

            #[allow(non_camel_case_types)]
            #[derive(Debug, Default, Clone)]
            pub struct #struct_ident {
                pub mode: #enum_ident,
            }

            impl #struct_ident {
                /// The Rx/Tx PDO-assignment index lists (0x1C12/0x1C13) for the
                /// active mode. (issue #70)
                #[must_use]
                pub fn pdo_assignment(&self) -> PdoAssignment<'static> {
                    #pdo_assignment_body
                }
            }

            pub const #const_ident: taktora_ethercat_esi_rt::Identity =
                taktora_ethercat_esi_rt::Identity {
                    vendor_id: #vendor_id,
                    product_code: #product_code,
                    revision: #revision,
                };

            impl taktora_ethercat_esi_rt::EsiDevice for #struct_ident {
                fn identity(&self) -> taktora_ethercat_esi_rt::Identity {
                    #const_ident
                }

                fn input_len(&self) -> usize {
                    #input_len
                }

                fn output_len(&self) -> usize {
                    #output_len
                }

                fn decode_inputs(
                    &mut self,
                    bits: &taktora_ethercat_esi_rt::BitSlice<u8, taktora_ethercat_esi_rt::Lsb0>,
                ) -> Result<(), taktora_ethercat_esi_rt::EsiError> {
                    #decode_body
                }

                fn encode_outputs(
                    &self,
                    bits: &mut taktora_ethercat_esi_rt::BitSlice<u8, taktora_ethercat_esi_rt::Lsb0>,
                ) -> Result<(), taktora_ethercat_esi_rt::EsiError> {
                    #encode_body
                }
            }
        })
    }

    fn emit_module_root(&self, devices: &[Device]) -> Result<TokenStream, CodegenError> {
        // A static table mapping each device's already-emitted identity const to
        // a non-capturing factory closure (which coerces to `fn() -> Box<dyn
        // EsiDevice>`), plus an O(n)-scan `device_for` lookup. The slice + linear
        // `find` is trivially reducible to a `HashMap<Identity, _>` lookup
        // downstream should device counts ever grow (`REQ_0525`). Entries follow
        // the resolved device order, which `generate` fixes order-independently.
        let entries = devices.iter().map(|device| {
            let struct_ident = &device.struct_ident;
            let const_ident = &device.const_ident;
            quote! {
                (
                    #const_ident,
                    || Box::new(#struct_ident::default())
                        as Box<dyn taktora_ethercat_esi_rt::EsiDevice>
                )
            }
        });

        Ok(quote! {
            /// Rx/Tx PDO-assignment index lists (0x1C12/0x1C13) for a device's
            /// active mode. Returned by each device's `pdo_assignment()`.
            #[derive(Debug, Clone, Copy)]
            pub struct PdoAssignment<'a> {
                pub rx: &'a [u16],
                pub tx: &'a [u16],
            }

            /// All devices generated in this module, keyed by EtherCAT identity.
            /// A linear scan over this slice is reducible to a `HashMap` lookup.
            pub static REGISTRY: &[(
                taktora_ethercat_esi_rt::Identity,
                fn() -> Box<dyn taktora_ethercat_esi_rt::EsiDevice>,
            )] = &[
                #(#entries,)*
            ];

            /// Construct a fresh device instance for the given identity, if known.
            pub fn device_for(
                identity: taktora_ethercat_esi_rt::Identity,
            ) -> Option<Box<dyn taktora_ethercat_esi_rt::EsiDevice>> {
                REGISTRY.iter().find(|(id, _)| *id == identity).map(|(_, make)| make())
            }
        })
    }
}

/// Emit one `pub` struct field declaration for a resolved entry, attaching its
/// width-inferred opaque doc-comment (if any) so the generated code documents
/// that the original semantic type was not modelled.
fn field_decl(f: &ResolvedField) -> TokenStream {
    let fi = &f.ident;
    let ty = &f.rust_type;
    let doc = f.doc.iter().map(|d| quote! { #[doc = #d] });
    quote! { #(#doc)* pub #fi: #ty }
}

/// Emit one PDO sub-struct: a plain `#[derive(Debug, Default, Clone)]` data
/// holder with one `pub` field per resolved entry, no trait impls.
fn emit_sub_struct(pdo: &ResolvedPdo) -> TokenStream {
    let ident = &pdo.struct_ident;
    let field_defs = pdo.fields.iter().map(field_decl);
    quote! {
        #[allow(non_camel_case_types)]
        #[derive(Debug, Default, Clone)]
        pub struct #ident {
            #(#field_defs,)*
        }
    }
}

/// Emit `<Dev><Variant>` = `{ inputs, outputs }` plus its two direction
/// sub-structs (issue #70).
fn emit_variant_types(a: &ResolvedAssignment) -> TokenStream {
    let in_ident = direction_struct_ident(&a.struct_ident, "In");
    let out_ident = direction_struct_ident(&a.struct_ident, "Out");
    let variant = &a.struct_ident;
    let in_defs = emit_direction_struct(&in_ident, &a.inputs);
    let out_defs = emit_direction_struct(&out_ident, &a.outputs);
    quote! {
        #in_defs
        #out_defs
        #[allow(non_camel_case_types)]
        #[derive(Debug, Default, Clone)]
        pub struct #variant {
            pub inputs: #in_ident,
            pub outputs: #out_ident,
        }
    }
}

/// `<Variant>In` / `<Variant>Out` direction-struct ident.
fn direction_struct_ident(variant: &Ident, suffix: &str) -> Ident {
    Ident::new(
        &format!("{variant}{suffix}"),
        proc_macro2::Span::call_site(),
    )
}

/// Emit one direction struct (flat fields when <=1 PDO, else one sub-struct
/// field per PDO) plus nested PDO sub-struct definitions.
fn emit_direction_struct(ident: &Ident, pdos: &[ResolvedPdo]) -> TokenStream {
    let flat = pdos.len() <= 1;
    let mut nested = TokenStream::new();
    if !flat {
        for p in pdos {
            nested.extend(emit_sub_struct(p));
        }
    }
    let fields = direction_device_fields(pdos, flat);
    quote! {
        #nested
        #[allow(non_camel_case_types)]
        #[derive(Debug, Default, Clone)]
        pub struct #ident {
            #fields
        }
    }
}

/// Emit the `<Dev>OpMode` enum + manual `Default` (first variant).
fn emit_op_mode_enum(enum_ident: &Ident, assigns: &[ResolvedAssignment]) -> TokenStream {
    let variants = assigns.iter().map(|a| {
        let v = &a.variant_ident;
        let s = &a.struct_ident;
        quote! { #v(#s) }
    });
    let first = &assigns[0].variant_ident;
    quote! {
        #[allow(non_camel_case_types)]
        #[derive(Debug, Clone)]
        pub enum #enum_ident {
            #(#variants,)*
        }
        impl Default for #enum_ident {
            fn default() -> Self { Self::#first(Default::default()) }
        }
    }
}

/// Emit the `decode_inputs` body: `match` the active mode, in each arm guard the
/// buffer length then read that mode's inputs.
fn emit_decode(enum_ident: &Ident, assigns: &[ResolvedAssignment]) -> TokenStream {
    let import = bitfield_import_for_assigns(assigns, |a| &a.inputs);
    let arms = assigns.iter().map(|a| {
        let v = &a.variant_ident;
        let guard = length_guard(a.input_bits);
        let reads = direction_member_stmts(&a.inputs, "inputs", read_member);
        let binding = if reads.is_empty() {
            quote!(_)
        } else {
            quote!(m)
        };
        quote! { #enum_ident::#v(#binding) => { #guard #(#reads)* } }
    });
    quote! {
        #import
        match &mut self.mode {
            #(#arms)*
        }
        Ok(())
    }
}

/// Emit the `encode_outputs` body: `match` the active mode (shared ref), in each
/// arm guard the buffer length then write that mode's outputs.
fn emit_encode(enum_ident: &Ident, assigns: &[ResolvedAssignment]) -> TokenStream {
    let import = bitfield_import_for_assigns(assigns, |a| &a.outputs);
    let arms = assigns.iter().map(|a| {
        let v = &a.variant_ident;
        let guard = length_guard(a.output_bits);
        let writes = direction_member_stmts(&a.outputs, "outputs", write_member);
        let binding = if writes.is_empty() {
            quote!(_)
        } else {
            quote!(m)
        };
        quote! { #enum_ident::#v(#binding) => { #guard #(#writes)* } }
    });
    quote! {
        #import
        match &self.mode {
            #(#arms)*
        }
        Ok(())
    }
}

/// Emit the per-mode `input_len`/`output_len` body (byte count of the active
/// mode's direction).
fn emit_len(enum_ident: &Ident, assigns: &[ResolvedAssignment], inputs: bool) -> TokenStream {
    let arms = assigns.iter().map(|a| {
        let v = &a.variant_ident;
        let bytes = (if inputs { a.input_bits } else { a.output_bits }).div_ceil(8);
        quote! { #enum_ident::#v(_) => #bytes }
    });
    quote! {
        match &self.mode {
            #(#arms,)*
        }
    }
}

/// Emit the `pdo_assignment()` body: per-mode Rx/Tx index lists.
fn emit_pdo_assignment(enum_ident: &Ident, assigns: &[ResolvedAssignment]) -> TokenStream {
    let arms = assigns.iter().map(|a| {
        let v = &a.variant_ident;
        let rx = &a.rx_indices;
        let tx = &a.tx_indices;
        quote! {
            #enum_ident::#v(_) => PdoAssignment {
                rx: &[#(#rx),*],
                tx: &[#(#tx),*],
            }
        }
    });
    quote! {
        match &self.mode {
            #(#arms,)*
        }
    }
}

/// Per-field read/write into a match-bound `m.<inputs|outputs>` member path.
fn direction_member_stmts(
    pdos: &[ResolvedPdo],
    member: &str,
    f: fn(&TokenStream, &ResolvedField) -> TokenStream,
) -> Vec<TokenStream> {
    let flat = pdos.len() <= 1;
    let member_ident = Ident::new(member, proc_macro2::Span::call_site());
    let mut out = Vec::new();
    for pdo in pdos {
        for field in &pdo.fields {
            let fi = &field.ident;
            let path = if flat {
                quote! { m.#member_ident.#fi }
            } else {
                let p = &pdo.field_ident;
                quote! { m.#member_ident.#p.#fi }
            };
            out.push(f(&path, field));
        }
    }
    out
}

/// Read a field out of `bits` into a match-bound member path.
fn read_member(path: &TokenStream, field: &ResolvedField) -> TokenStream {
    read_into(path, field)
}

/// Write a field from a match-bound member path into `bits`.
fn write_member(path: &TokenStream, field: &ResolvedField) -> TokenStream {
    write_from(path, field)
}

/// Emit the `BitField` trait import iff any field in the selected direction of
/// any assignment needs a multi-bit `load_le`/`store_le`.
fn bitfield_import_for_assigns(
    assigns: &[ResolvedAssignment],
    dir: fn(&ResolvedAssignment) -> &Vec<ResolvedPdo>,
) -> TokenStream {
    let needs = assigns.iter().any(|a| {
        dir(a)
            .iter()
            .flat_map(|p| p.fields.iter())
            .any(|f| layout_uses_bitfield(f.layout))
    });
    if needs {
        quote! { use bitvec::field::BitField as _; }
    } else {
        TokenStream::new()
    }
}

/// Whether a layout's decode/encode emits a `load_le`/`store_le` call (and so
/// needs the `bitvec` `BitField` trait in scope). Both multi-bit scalar fields
/// and opaque byte arrays do; single-bit `Bool`s do not.
const fn layout_uses_bitfield(layout: Layout) -> bool {
    matches!(layout, Layout::Field { .. } | Layout::Bytes { .. })
}

/// The device-struct field declarations for one direction: flat → one field per
/// entry; split → one `pub <pdo>: <SubStruct>` per PDO.
fn direction_device_fields(pdos: &[ResolvedPdo], flat: bool) -> TokenStream {
    if flat {
        let defs = pdos.iter().flat_map(|p| p.fields.iter()).map(|f| {
            let decl = field_decl(f);
            quote! { #decl, }
        });
        quote! { #(#defs)* }
    } else {
        let defs = pdos.iter().map(|p| {
            let field = &p.field_ident;
            let ty = &p.struct_ident;
            quote! { pub #field: #ty, }
        });
        quote! { #(#defs)* }
    }
}

/// Build the `BufferTooShort` length guard for a given total bit count.
fn length_guard(total_bits: usize) -> TokenStream {
    quote! {
        const NEED: usize = #total_bits;
        if bits.len() < NEED {
            return Err(taktora_ethercat_esi_rt::EsiError::BufferTooShort {
                expected_bits: NEED,
                got_bits: bits.len(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use taktora_ethercat_esi::DataType;

    fn ty(dt: Option<&DataType>, bits: u16) -> String {
        let (ft, _) = typemap::resolve(dt, bits, 0, 0x6000, 1, "f").expect("supported");
        ft.rust_type.to_string()
    }

    #[test]
    fn type_map_scalar_types() {
        assert_eq!(ty(Some(&DataType::Bool), 1), "bool");
        assert_eq!(ty(Some(&DataType::I16), 16), "i16");
        assert_eq!(ty(Some(&DataType::U8), 8), "u8");
        assert_eq!(ty(Some(&DataType::Real32), 32), "f32");
        assert_eq!(ty(Some(&DataType::Real64), 64), "f64");
    }

    #[test]
    fn type_map_bitn_rounds_to_next_uint() {
        assert_eq!(ty(Some(&DataType::BitN(3)), 3), "u8");
        assert_eq!(ty(Some(&DataType::BitN(8)), 8), "u8");
    }

    #[test]
    fn type_map_bit2_is_sub_byte_u8() {
        // BIT2 → 2-bit field stored in a u8, loaded via load_le with masking.
        let (ft, layout) =
            typemap::resolve(Some(&DataType::BitN(2)), 2, 5, 0x6000, 3, "limit_1").expect("BIT2");
        assert_eq!(ft.rust_type.to_string(), "u8");
        match layout {
            Layout::Field {
                offset,
                width,
                kind,
            } => {
                assert_eq!(offset, 5);
                assert_eq!(width, 2);
                assert_eq!(kind, ScalarKind::Int);
            }
            other => panic!("expected sub-byte Field layout, got {other:?}"),
        }
    }

    #[test]
    fn type_map_dint_is_i32_over_32_bits() {
        // DINT parses to DataType::I32 → exact 32-bit signed.
        let (ft, layout) =
            typemap::resolve(Some(&DataType::I32), 32, 16, 0x6000, 17, "value").expect("DINT");
        assert_eq!(ft.rust_type.to_string(), "i32");
        match layout {
            Layout::Field { width, kind, .. } => {
                assert_eq!(width, 32);
                assert_eq!(kind, ScalarKind::Int);
            }
            other => panic!("expected 32-bit Field layout, got {other:?}"),
        }
    }

    #[test]
    fn type_map_untyped_width_inferred() {
        assert_eq!(ty(None, 1), "bool");
        assert_eq!(ty(None, 12), "u16");
    }

    /// Q7 reversal: strings no longer abort codegen. A sized string entry maps
    /// to a width-inferred opaque unsigned field carrying a doc marker.
    #[test]
    fn type_map_visible_string_is_opaque_width_inferred() {
        let (ft, layout) =
            typemap::resolve(Some(&DataType::VisibleString), 16, 0, 0x6000, 5, "name")
                .expect("strings now resolve");
        assert_eq!(ft.rust_type.to_string(), "u16");
        assert!(
            ft.doc.as_deref().is_some_and(|d| d.contains("opaque")
                && d.contains("VisibleString")
                && d.contains("16 bits")),
            "expected opaque doc marker, got {:?}",
            ft.doc
        );
        assert!(matches!(layout, Layout::Field { width: 16, .. }));
    }

    /// Q7 reversal: Beckhoff `BITARR8` (`Named`, 8 bits) → opaque `u8`.
    #[test]
    fn type_map_named_bitarr8_is_opaque_u8() {
        let dt = DataType::Named("BITARR8".to_owned());
        let (ft, _) = typemap::resolve(Some(&dt), 8, 0, 0x6000, 5, "arr").expect("named resolves");
        assert_eq!(ft.rust_type.to_string(), "u8");
        assert!(
            ft.doc
                .as_deref()
                .is_some_and(|d| d.contains("opaque") && d.contains("BITARR8")),
            "expected opaque BITARR8 doc, got {:?}",
            ft.doc
        );
    }

    /// `Named` `BITARR16` (16 bits) → opaque `u16`.
    #[test]
    fn type_map_named_bitarr16_is_opaque_u16() {
        let dt = DataType::Named("BITARR16".to_owned());
        let (ft, _) = typemap::resolve(Some(&dt), 16, 0, 0x6000, 5, "arr").expect("named resolves");
        assert_eq!(ft.rust_type.to_string(), "u16");
    }

    /// A 24-bit `Named` → opaque `u32` (next-larger unsigned, masked load).
    #[test]
    fn type_map_named_24bit_is_opaque_u32_masked() {
        let dt = DataType::Named("DT2008".to_owned());
        let (ft, layout) =
            typemap::resolve(Some(&dt), 24, 0, 0x6000, 5, "x").expect("named resolves");
        assert_eq!(ft.rust_type.to_string(), "u32");
        match layout {
            Layout::Field { width, kind, .. } => {
                assert_eq!(width, 24);
                assert_eq!(kind, ScalarKind::Int);
            }
            other => panic!("expected masked Field layout, got {other:?}"),
        }
    }

    /// A 1-bit `Named`/`BitN` → bool (width takes precedence over the type name).
    #[test]
    fn type_map_one_bit_opaque_is_bool() {
        let dt = DataType::Named("BITARR8".to_owned());
        let (ft, layout) =
            typemap::resolve(Some(&dt), 1, 7, 0x6000, 5, "b").expect("named resolves");
        assert_eq!(ft.rust_type.to_string(), "bool");
        assert!(matches!(layout, Layout::Bool { offset: 7 }));

        let (ft2, _) = typemap::resolve(Some(&DataType::BitN(1)), 1, 0, 0x6000, 5, "b2")
            .expect("bitn resolves");
        assert_eq!(ft2.rust_type.to_string(), "bool");
    }

    /// A >64-bit opaque entry → fixed byte array `[u8; N]` (N = ceil(bits/8)).
    #[test]
    fn type_map_over_64_bits_is_byte_array() {
        let dt = DataType::Named("DT8000".to_owned());
        let (ft, layout) =
            typemap::resolve(Some(&dt), 72, 0, 0x6000, 5, "blob").expect("wide resolves");
        assert_eq!(ft.rust_type.to_string(), "[u8 ; 9usize]");
        match layout {
            Layout::Bytes { offset, bytes } => {
                assert_eq!(offset, 0);
                assert_eq!(bytes, 9);
            }
            other => panic!("expected Bytes layout, got {other:?}"),
        }
    }

    /// A zero-width non-padding entry is genuinely unusable → error. (Padding
    /// entries with index 0 never reach `resolve`.)
    #[test]
    fn type_map_zero_width_named_is_unsupported() {
        let dt = DataType::Named("WEIRD".to_owned());
        let err =
            typemap::resolve(Some(&dt), 0, 0, 0x6000, 5, "z").expect_err("zero width unusable");
        assert!(matches!(err, CodegenError::UnsupportedEntryType { .. }));
    }

    use taktora_ethercat_esi::{Pdo, PdoEntry};

    fn entry(index: u16, sub: u8, bits: u16, name: Option<&str>, dt: DataType) -> PdoEntry {
        PdoEntry {
            index,
            sub_index: sub,
            bit_length: bits,
            name: name.map(ToOwned::to_owned),
            data_type: Some(dt),
        }
    }

    fn pdo(entries: Vec<PdoEntry>) -> Pdo {
        Pdo {
            index: 0x1A00,
            name: Some("P".to_owned()),
            sm: None,
            fixed: false,
            mandatory: false,
            exclude: Vec::new(),
            entries,
        }
    }

    /// Two entries inside one PDO that snake-case to the same field name are
    /// disambiguated deterministically with a numeric suffix.
    #[test]
    fn intra_pdo_field_names_are_deduped() {
        let p = pdo(vec![
            entry(0x6000, 1, 8, Some("Status"), DataType::U8),
            entry(0x6000, 2, 8, Some("Status"), DataType::U8),
            entry(0x6000, 3, 8, Some("Status"), DataType::U8),
        ]);
        let mut offset = 0usize;
        let fields = resolve_pdo_fields(&p, &mut offset).expect("resolve");
        let idents: Vec<String> = fields.iter().map(|f| f.ident.to_string()).collect();
        assert_eq!(idents, vec!["status", "status_2", "status_3"]);
        assert_eq!(offset, 24);
    }

    /// An unnamed non-padding entry (`name: None`, index != 0) gets a synthetic
    /// `entry_<index:04x>_<sub>` field name.
    #[test]
    fn unnamed_entry_gets_synthetic_field_name() {
        let p = pdo(vec![entry(0x7000, 5, 16, None, DataType::U16)]);
        let mut offset = 0usize;
        let fields = resolve_pdo_fields(&p, &mut offset).expect("resolve");
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].ident.to_string(), "entry_7000_5");
        assert_eq!(offset, 16);
    }

    /// A padding entry (`index == 0`) advances the running offset but emits no
    /// field.
    #[test]
    fn padding_entry_advances_offset_without_field() {
        let p = pdo(vec![
            entry(0x6000, 1, 1, Some("Flag"), DataType::Bool),
            PdoEntry {
                index: 0,
                sub_index: 0,
                bit_length: 7,
                name: None,
                data_type: None,
            },
            entry(0x6000, 2, 8, Some("Byte"), DataType::U8),
        ]);
        let mut offset = 0usize;
        let fields = resolve_pdo_fields(&p, &mut offset).expect("resolve");
        let idents: Vec<String> = fields.iter().map(|f| f.ident.to_string()).collect();
        assert_eq!(idents, vec!["flag", "byte"]);
        // 1 (flag) + 7 (pad) + 8 (byte) = 16.
        assert_eq!(offset, 16);
        // The byte field lands at offset 8 (after the pad).
        assert!(matches!(fields[1].layout, Layout::Field { offset: 8, .. }));
    }
}
