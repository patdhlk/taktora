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
    CodegenBackend, CodegenError, Device, Identity, field_ident, pdo_assignment_enum_ident,
    pdo_assignment_field_ident, pdo_field_ident, pdo_struct_ident, pdo_variant_ident,
    pdo_variant_struct_ident,
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

/// One alternative PDO inside an alternative group: its enum-variant ident, its
/// per-variant struct ident, the resolved entry fields, and the variant's total
/// bit width (used for the active-variant length guard).
struct ResolvedAlternative {
    variant_ident: Ident,
    struct_ident: Ident,
    fields: Vec<ResolvedField>,
    bits: usize,
}

/// One alternative group within a direction: the device-struct enum field, the
/// enum type ident, and the closed set of alternatives the master chooses among
/// at bring-up (0x1C12/0x1C13). Exactly one alternative is active at runtime.
struct ResolvedAltGroup {
    field_ident: Ident,
    enum_ident: Ident,
    alternatives: Vec<ResolvedAlternative>,
}

/// Split a direction's PDOs into the always-on set (`Fixed || Mandatory`, kept
/// as the existing T8 flat/sub-struct treatment) and the alternative candidates
/// (everything else — the master picks one at bring-up, so they form a closed
/// CHOICE). Declaration order is preserved within each partition.
fn partition_pdos(pdos: &[Pdo]) -> (Vec<&Pdo>, Vec<&Pdo>) {
    pdos.iter().partition(|p| p.fixed || p.mandatory)
}

/// Classify a whole direction's PDOs into the final always-on set and the
/// genuine (≥2-PDO) alternative groups.
///
/// The always-on set is `(Fixed || Mandatory)` PDOs ∪ singleton candidates (a
/// non-fixed/non-mandatory PDO that is the only candidate in its Sm/`<Exclude>`
/// group is not an alternative — see [`classify_alternatives`]). It is returned
/// in original declaration order so the running bit-offset threads correctly:
/// every always-on PDO — reclassified singleton or not — takes its place in the
/// offset accumulator in declaration order.
fn classify_direction(pdos: &[Pdo]) -> (Vec<&Pdo>, Vec<Vec<&Pdo>>) {
    let (_, candidates) = partition_pdos(pdos);
    let (singletons, genuine) = classify_alternatives(&candidates);

    let is_singleton = |p: &Pdo| singletons.iter().any(|s| std::ptr::eq(*s, p));
    let always_on: Vec<&Pdo> = pdos
        .iter()
        .filter(|p| p.fixed || p.mandatory || is_singleton(p))
        .collect();

    (always_on, genuine)
}

/// Union-find `find` with path-halving over a `parent` slice.
fn uf_find(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

/// Union the components containing `a` and `b`.
fn uf_union(parent: &mut [usize], a: usize, b: usize) {
    let (ra, rb) = (uf_find(parent, a), uf_find(parent, b));
    if ra != rb {
        parent[ra] = rb;
    }
}

/// Group the alternative-candidate PDOs into alternative groups: PDOs sharing a
/// sync manager (`pdo.sm`) form one group, then `<Exclude>` connected
/// components refine the grouping (two PDOs in the same SM that exclude each
/// other, transitively, end in the same group). PDOs with no `sm` each form
/// their own singleton group keyed by index, so they never merge spuriously.
///
/// Returns groups in first-appearance order; within a group, PDOs stay in
/// declaration order.
fn group_alternatives<'a>(candidates: &[&'a Pdo]) -> Vec<Vec<&'a Pdo>> {
    // Union-find over the candidate slice indices.
    let mut parent: Vec<usize> = (0..candidates.len()).collect();

    // 1) Union PDOs that share a (Some) sync manager.
    for (i, pi) in candidates.iter().enumerate() {
        for (offset, pj) in candidates[i + 1..].iter().enumerate() {
            if let (Some(si), Some(sj)) = (pi.sm, pj.sm) {
                if si == sj {
                    uf_union(&mut parent, i, i + 1 + offset);
                }
            }
        }
    }

    // 2) Refine with <Exclude> edges (by mapping index), unioning the endpoints.
    let index_of: std::collections::HashMap<u16, usize> = candidates
        .iter()
        .enumerate()
        .map(|(i, p)| (p.index, i))
        .collect();
    for (i, p) in candidates.iter().enumerate() {
        for excl in &p.exclude {
            if let Some(&j) = index_of.get(excl) {
                uf_union(&mut parent, i, j);
            }
        }
    }

    // Collect components in first-appearance order.
    let mut order: Vec<usize> = Vec::new();
    let mut groups: std::collections::HashMap<usize, Vec<&Pdo>> = std::collections::HashMap::new();
    for (i, pdo) in candidates.iter().enumerate() {
        let root = uf_find(&mut parent, i);
        if !groups.contains_key(&root) {
            order.push(root);
        }
        groups.entry(root).or_default().push(pdo);
    }
    order
        .into_iter()
        .map(|r| groups.remove(&r).expect("root collected above"))
        .collect()
}

/// Refine the alternative-candidate grouping into real alternatives vs.
/// mis-grouped always-on PDOs.
///
/// A candidate group of size 1 is NOT an alternative: a non-`Fixed`/non-
/// `Mandatory` PDO that is the only candidate competing for its sync manager
/// (or `<Exclude>` component) is just an always-on (default) PDO whose mapping
/// happens to be reconfigurable. Only a group of **≥2** candidates is a genuine
/// alternative set (mutually-exclusive mappings competing for one Sm, or
/// `<Exclude>`-linked).
///
/// Returns `(singletons, genuine_groups)`:
/// - `singletons`: the lone-candidate PDOs to fold back into the always-on set,
///   in declaration order;
/// - `genuine_groups`: the ≥2-PDO groups, in first-appearance order, each in
///   declaration order.
fn classify_alternatives<'a>(candidates: &[&'a Pdo]) -> (Vec<&'a Pdo>, Vec<Vec<&'a Pdo>>) {
    let groups = group_alternatives(candidates);
    let mut singletons: Vec<&Pdo> = Vec::new();
    let mut genuine: Vec<Vec<&Pdo>> = Vec::new();
    for group in groups {
        if group.len() == 1 {
            singletons.push(group[0]);
        } else {
            genuine.push(group);
        }
    }
    // Preserve declaration order among reclassified singletons (the
    // first-appearance group order already follows declaration order, but a
    // group's first member is not necessarily its lowest declaration index, so
    // sort defensively by the candidate-slice position).
    singletons.sort_by_key(|p| {
        candidates
            .iter()
            .position(|c| std::ptr::eq(*c, *p))
            .unwrap_or(usize::MAX)
    });
    (singletons, genuine)
}

/// Resolve one alternative group into [`ResolvedAlternative`]s. Every
/// alternative's entries are laid out starting at `base_offset` (the running
/// offset after this direction's always-on PDOs), since exactly one alternative
/// occupies that space at runtime. `label` disambiguates the enum/field idents
/// across groups; the caller passes `None` for the single-group case (the only
/// shape currently emitted — see [`resolve_alt_groups`]).
fn resolve_alt_group(
    group: &[&Pdo],
    device_struct: &Ident,
    base_offset: usize,
    label: Option<&str>,
) -> Result<ResolvedAltGroup, CodegenError> {
    let mut alternatives = Vec::with_capacity(group.len());
    for pdo in group {
        let mut offset = base_offset;
        let fields = resolve_pdo_fields(pdo, &mut offset)?;
        alternatives.push(ResolvedAlternative {
            variant_ident: pdo_variant_ident(pdo.name.as_deref(), pdo.index)?,
            struct_ident: pdo_variant_struct_ident(device_struct, pdo.name.as_deref(), pdo.index)?,
            fields,
            bits: offset - base_offset,
        });
    }
    Ok(ResolvedAltGroup {
        field_ident: pdo_assignment_field_ident(label)?,
        enum_ident: pdo_assignment_enum_ident(device_struct, label)?,
        alternatives,
    })
}

/// Resolve a direction's genuine alternative groups. Always-on PDOs (already
/// resolved) occupy `base_offset` bits; the alternative group's variants start
/// there.
///
/// `genuine` carries only ≥2-PDO groups (singleton candidates were already
/// reclassified as always-on by [`classify_direction`]). The spec-required
/// shape is at most one genuine group per direction (the master selects a
/// single PDO assignment via 0x1C12/0x1C13), so the single group is unlabelled
/// (bare `pdo` / `<Dev>PdoAssignment`). A direction with MORE THAN ONE genuine
/// group is rejected with [`CodegenError::MultipleAlternativeGroups`] rather
/// than miscompiled (see the inline `TODO`).
fn resolve_alt_groups(
    genuine: &[Vec<&Pdo>],
    device_struct: &Ident,
    base_offset: usize,
    direction: &'static str,
) -> Result<Vec<ResolvedAltGroup>, CodegenError> {
    // TODO: support more than one alternative group per direction by sequencing
    // each group's `base_offset` after the previous group's widest variant
    // (instead of giving every group the same `base_offset`). Until then a
    // multi-group direction is rejected: `direction_len` SUMS the widest-per-
    // group while `resolve_alt_group` lays every group at the same offset, so
    // two groups would alias the same bits (silent data corruption).
    if genuine.len() > 1 {
        return Err(CodegenError::MultipleAlternativeGroups {
            device: device_struct.to_string(),
            direction,
        });
    }
    // Single genuine group (the only spec-required case) stays unlabelled: bare
    // `pdo` / `<Dev>PdoAssignment`.
    genuine
        .iter()
        .map(|group| resolve_alt_group(group, device_struct, base_offset, None))
        .collect()
}

/// The `self.<…>` access path for a field: bare `self.<field>` in the flat
/// shape, or `self.<pdo>.<field>` when the direction is split into per-PDO
/// sub-structs.
fn access_path(pdo_field: Option<&Ident>, field: &Ident) -> TokenStream {
    pdo_field.map_or_else(|| quote! { self.#field }, |pdo| quote! { self.#pdo.#field })
}

/// Build the per-field read expression assigning out of `bits` into the field's
/// access path (`self.<field>` or `self.<pdo>.<field>`).
fn read_stmt(pdo_field: Option<&Ident>, field: &ResolvedField) -> TokenStream {
    read_into(&access_path(pdo_field, &field.ident), field)
}

/// Build the per-field write expression storing the field's access path into
/// `bits`.
fn write_stmt(pdo_field: Option<&Ident>, field: &ResolvedField) -> TokenStream {
    write_from(&access_path(pdo_field, &field.ident), field)
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

        // Classify each direction: the always-on set is `Fixed || Mandatory`
        // PDOs PLUS any non-fixed/non-mandatory PDO that is the lone candidate
        // in its sync-manager (or `<Exclude>`) group — such a singleton is not
        // an alternative, just a default PDO whose mapping is reconfigurable.
        // Only a group of ≥2 genuinely-competing PDOs becomes a closed sum type
        // the master chooses among at bring-up (0x1C12/0x1C13; REQ_0523/0524).
        let (in_always_on, in_genuine) = classify_direction(device.tx_pdos);
        let (out_always_on, out_genuine) = classify_direction(device.rx_pdos);

        let (inputs, input_base_bits) = resolve_direction(&in_always_on, struct_ident)?;
        let (outputs, output_base_bits) = resolve_direction(&out_always_on, struct_ident)?;

        let in_groups = resolve_alt_groups(&in_genuine, struct_ident, input_base_bits, "Tx")?;
        let out_groups = resolve_alt_groups(&out_genuine, struct_ident, output_base_bits, "Rx")?;

        // A direction with more than one always-on PDO is split into per-PDO
        // sub-structs so that entry names repeated across channels (e.g. each
        // EL2004 channel's `Output`) no longer collide; a single-PDO direction
        // stays flat, keeping the bullet-1 device byte-identical.
        let inputs_flat = inputs.len() <= 1;
        let outputs_flat = outputs.len() <= 1;

        // Sub-struct type definitions for every split always-on direction.
        let mut sub_structs = TokenStream::new();
        for pdo in &inputs {
            if !inputs_flat {
                sub_structs.extend(emit_sub_struct(pdo));
            }
        }
        for pdo in &outputs {
            if !outputs_flat {
                sub_structs.extend(emit_sub_struct(pdo));
            }
        }

        // Per-variant structs and the choice enum (+ manual Default) for every
        // alternative group, in both directions.
        let mut alt_defs = TokenStream::new();
        for group in in_groups.iter().chain(&out_groups) {
            alt_defs.extend(emit_alt_group_types(group)?);
        }

        // Device-struct fields: flat directions contribute their entry fields
        // directly; split directions contribute one sub-struct field per PDO;
        // each alternative group contributes one enum-typed field.
        let mut device_fields = TokenStream::new();
        device_fields.extend(direction_device_fields(&inputs, inputs_flat));
        device_fields.extend(direction_device_fields(&outputs, outputs_flat));
        for group in in_groups.iter().chain(&out_groups) {
            let field = &group.field_ident;
            let ty = &group.enum_ident;
            device_fields.extend(quote! { pub #field: #ty, });
        }

        let Identity {
            vendor_id,
            product_code,
            revision,
        } = device.identity;

        // Lengths: the always-on portion is fixed; an alternative group's
        // contribution is the active variant's width, reported at runtime via
        // the variant's own length guard. `input_len`/`output_len` advertise the
        // largest layout so the rt allocates a sufficient buffer.
        let input_len = direction_len(input_base_bits, &in_groups);
        let output_len = direction_len(output_base_bits, &out_groups);

        let decode_body = decode_body(&inputs, inputs_flat, &in_groups, input_base_bits);
        let encode_body = encode_body(&outputs, outputs_flat, &out_groups, output_base_bits);

        Ok(quote! {
            #sub_structs

            #alt_defs

            #[allow(non_camel_case_types)]
            #[derive(Debug, Default, Clone)]
            pub struct #struct_ident {
                #device_fields
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

/// Emit the type definitions for one alternative group (`REQ_0523`/`REQ_0524`):
///
/// 1. one `<Dev>Pdo<Variant>` data struct per alternative, holding that
///    variant's typed entries;
/// 2. the `<Dev>PdoAssignment[<Label>]` choice enum, one tuple variant per
///    alternative embedding its struct — so "two alternatives at once" is
///    unrepresentable (`ADR_0072`);
/// 3. a manual `impl Default` selecting the first alternative (the enum has
///    non-unit variants, so `#[derive(Default)]` is unavailable).
fn emit_alt_group_types(group: &ResolvedAltGroup) -> Result<TokenStream, CodegenError> {
    let enum_ident = &group.enum_ident;

    // Per-variant structs.
    let mut structs = TokenStream::new();
    for alt in &group.alternatives {
        let ident = &alt.struct_ident;
        let field_defs = alt.fields.iter().map(field_decl);
        structs.extend(quote! {
            #[allow(non_camel_case_types)]
            #[derive(Debug, Default, Clone)]
            pub struct #ident {
                #(#field_defs,)*
            }
        });
    }

    // Enum variants embedding each per-variant struct.
    let variants = group.alternatives.iter().map(|alt| {
        let v = &alt.variant_ident;
        let s = &alt.struct_ident;
        quote! { #v(#s) }
    });

    // Manual Default → first declared alternative.
    let first = group
        .alternatives
        .first()
        .ok_or_else(|| CodegenError::EmptyAlternativeGroup {
            enum_ident: enum_ident.to_string(),
        })?;
    let first_variant = &first.variant_ident;

    Ok(quote! {
        #structs

        #[allow(non_camel_case_types)]
        #[derive(Debug, Clone)]
        pub enum #enum_ident {
            #(#variants,)*
        }

        impl Default for #enum_ident {
            fn default() -> Self {
                Self::#first_variant(Default::default())
            }
        }
    })
}

/// Emit the `decode_inputs` block for one input alternative group: `match` the
/// enum field, and in each arm read that variant's entries (already laid out at
/// the post-always-on `base_offset`) into the bound inner struct, after an
/// active-variant length guard.
fn emit_alt_decode(group: &ResolvedAltGroup, base_offset: usize) -> TokenStream {
    let field = &group.field_ident;
    let enum_ident = &group.enum_ident;
    let arms = group.alternatives.iter().map(|alt| {
        let v = &alt.variant_ident;
        let guard = length_guard(base_offset + alt.bits);
        let reads: Vec<TokenStream> = alt
            .fields
            .iter()
            .map(|f| {
                let fi = &f.ident;
                read_into(&quote! { v.#fi }, f)
            })
            .collect();
        quote! {
            #enum_ident::#v(v) => {
                #guard
                #(#reads)*
            }
        }
    });
    quote! {
        match &mut self.#field {
            #(#arms)*
        }
    }
}

/// Emit the `encode_outputs` block for one output alternative group: `match` the
/// enum field (by shared ref), and in each arm write that variant's entries from
/// the bound inner struct, after an active-variant length guard.
fn emit_alt_encode(group: &ResolvedAltGroup, base_offset: usize) -> TokenStream {
    let field = &group.field_ident;
    let enum_ident = &group.enum_ident;
    let arms = group.alternatives.iter().map(|alt| {
        let v = &alt.variant_ident;
        let guard = length_guard(base_offset + alt.bits);
        let writes: Vec<TokenStream> = alt
            .fields
            .iter()
            .map(|f| {
                let fi = &f.ident;
                write_from(&quote! { v.#fi }, f)
            })
            .collect();
        quote! {
            #enum_ident::#v(v) => {
                #guard
                #(#writes)*
            }
        }
    });
    quote! {
        match &self.#field {
            #(#arms)*
        }
    }
}

/// Assemble the `decode_inputs` body: the always-on length guard, the always-on
/// reads, then one `match` block per input alternative group. The `BitField`
/// import is emitted iff any read is a multi-bit `load_le`.
fn decode_body(
    inputs: &[ResolvedPdo],
    inputs_flat: bool,
    in_groups: &[ResolvedAltGroup],
    input_base_bits: usize,
) -> TokenStream {
    let read_stmts = direction_stmts(inputs, inputs_flat, read_stmt);
    let alt_reads = in_groups
        .iter()
        .map(|g| emit_alt_decode(g, input_base_bits));
    let import = bitfield_import_with_alts(inputs, in_groups);
    let guard = length_guard(input_base_bits);
    quote! {
        #import
        #guard
        #(#read_stmts)*
        #(#alt_reads)*
        Ok(())
    }
}

/// Assemble the `encode_outputs` body. A device with no `RxPdo` and no output
/// alternative group emits a no-op (suppressing the unused-`bits` warning);
/// otherwise the always-on guard + writes, then one `match` per output group.
fn encode_body(
    outputs: &[ResolvedPdo],
    outputs_flat: bool,
    out_groups: &[ResolvedAltGroup],
    output_base_bits: usize,
) -> TokenStream {
    if outputs.is_empty() && out_groups.is_empty() {
        return quote! { let _ = bits; Ok(()) };
    }
    let write_stmts = direction_stmts(outputs, outputs_flat, write_stmt);
    let alt_writes = out_groups
        .iter()
        .map(|g| emit_alt_encode(g, output_base_bits));
    let import = bitfield_import_with_alts(outputs, out_groups);
    let guard = length_guard(output_base_bits);
    quote! {
        #import
        #guard
        #(#write_stmts)*
        #(#alt_writes)*
        Ok(())
    }
}

/// The advertised byte length for one direction: the always-on base plus the
/// widest alternative across all groups (the rt allocates the largest layout;
/// the active-variant guard checks the actual need at decode/encode time).
fn direction_len(base_bits: usize, groups: &[ResolvedAltGroup]) -> usize {
    let alt_bits: usize = groups
        .iter()
        .map(|g| g.alternatives.iter().map(|a| a.bits).max().unwrap_or(0))
        .sum();
    (base_bits + alt_bits).div_ceil(8)
}

/// Whether a layout's decode/encode emits a `load_le`/`store_le` call (and so
/// needs the `bitvec` `BitField` trait in scope). Both multi-bit scalar fields
/// and opaque byte arrays do; single-bit `Bool`s do not.
const fn layout_uses_bitfield(layout: Layout) -> bool {
    matches!(layout, Layout::Field { .. } | Layout::Bytes { .. })
}

/// Whether any alternative across the given groups has a field needing the
/// `BitField` import.
fn alt_groups_need_bitfield(groups: &[ResolvedAltGroup]) -> bool {
    groups.iter().any(|g| {
        g.alternatives
            .iter()
            .flat_map(|a| a.fields.iter())
            .any(|f| layout_uses_bitfield(f.layout))
    })
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

/// The per-field read/write statements for one direction, choosing the access
/// path: flat → `self.<field>`; split → `self.<pdo>.<field>`.
fn direction_stmts(
    pdos: &[ResolvedPdo],
    flat: bool,
    stmt: fn(Option<&Ident>, &ResolvedField) -> TokenStream,
) -> Vec<TokenStream> {
    let mut out = Vec::new();
    for pdo in pdos {
        let prefix = if flat { None } else { Some(&pdo.field_ident) };
        for field in &pdo.fields {
            out.push(stmt(prefix, field));
        }
    }
    out
}

/// Emit the `BitField` trait import iff either the always-on PDOs or any
/// alternative group needs a multi-bit `load_le`/`store_le`. A single `use`
/// covers the whole body (always-on stmts and every match arm).
fn bitfield_import_with_alts(pdos: &[ResolvedPdo], groups: &[ResolvedAltGroup]) -> TokenStream {
    let needs = pdos
        .iter()
        .flat_map(|p| p.fields.iter())
        .any(|f| layout_uses_bitfield(f.layout))
        || alt_groups_need_bitfield(groups);
    if needs {
        quote! { use bitvec::field::BitField as _; }
    } else {
        TokenStream::new()
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

    // -----------------------------------------------------------------------
    // Grouping rule (T9, REQ_0523/0524): always-on vs alternatives, and how
    // alternative candidates partition into groups.
    // -----------------------------------------------------------------------

    /// A fully-specified PDO for grouping tests.
    fn alt_pdo(
        index: u16,
        name: &str,
        sm: Option<u8>,
        fixed: bool,
        mandatory: bool,
        exclude: Vec<u16>,
    ) -> Pdo {
        Pdo {
            index,
            name: Some(name.to_owned()),
            sm,
            fixed,
            mandatory,
            exclude,
            entries: vec![entry(0x6000, 1, 8, Some("Value"), DataType::U8)],
        }
    }

    /// `Fixed` OR `Mandatory` PDOs are always-on (kept as the existing flat/
    /// sub-struct treatment); only non-fixed, non-mandatory PDOs are alternative
    /// candidates.
    #[test]
    fn partition_splits_fixed_or_mandatory_from_alternatives() {
        let pdos = vec![
            alt_pdo(0x1A00, "Fixed", Some(3), true, false, vec![]),
            alt_pdo(0x1A01, "Mandatory", Some(3), false, true, vec![]),
            alt_pdo(0x1A02, "AltA", Some(3), false, false, vec![]),
            alt_pdo(0x1A03, "AltB", Some(3), false, false, vec![]),
        ];
        let (always_on, candidates) = partition_pdos(&pdos);
        let names = |v: &[&Pdo]| -> Vec<u16> { v.iter().map(|p| p.index).collect() };
        assert_eq!(names(&always_on), vec![0x1A00, 0x1A01]);
        assert_eq!(names(&candidates), vec![0x1A02, 0x1A03]);
    }

    /// Two non-fixed, non-mandatory PDOs sharing a sync manager form ONE
    /// alternative group (the synthetic ALT case).
    #[test]
    fn shared_sm_candidates_form_one_group() {
        let pdos = vec![
            alt_pdo(0x1A00, "Standard", Some(3), false, false, vec![]),
            alt_pdo(0x1A01, "Compact", Some(3), false, false, vec![]),
        ];
        let (_, candidates) = partition_pdos(&pdos);
        let groups = group_alternatives(&candidates);
        assert_eq!(groups.len(), 1, "shared Sm should yield a single group");
        assert_eq!(groups[0].len(), 2);
    }

    /// Candidates on distinct sync managers (and no `<Exclude>` linking them)
    /// land in separate alternative groups.
    #[test]
    fn distinct_sm_candidates_form_separate_groups() {
        let pdos = vec![
            alt_pdo(0x1A00, "InA", Some(3), false, false, vec![]),
            alt_pdo(0x1A01, "InB", Some(3), false, false, vec![]),
            alt_pdo(0x1600, "OutA", Some(2), false, false, vec![]),
        ];
        let (_, candidates) = partition_pdos(&pdos);
        let groups = group_alternatives(&candidates);
        assert_eq!(groups.len(), 2, "two SMs → two groups");
    }

    /// A direction that resolves to MORE THAN ONE alternative group is rejected
    /// with [`CodegenError::MultipleAlternativeGroups`] rather than miscompiled.
    /// Shape: two non-fixed/non-mandatory PDOs on Sm 3 linked by `<Exclude>`
    /// (group A) plus two more on Sm 4 with no cross-exclude (group B) — two
    /// distinct SM groups in the same Tx direction.
    #[test]
    fn multiple_alt_groups_in_one_direction_are_rejected() {
        let tx_pdos = vec![
            // Group A: Sm 3, A0 <-> A1 exclude each other.
            alt_pdo(0x1A00, "A0", Some(3), false, false, vec![0x1A01]),
            alt_pdo(0x1A01, "A1", Some(3), false, false, vec![0x1A00]),
            // Group B: Sm 4, no cross-exclude into group A.
            alt_pdo(0x1A10, "B0", Some(4), false, false, vec![]),
            alt_pdo(0x1A11, "B1", Some(4), false, false, vec![]),
        ];
        // Sanity: the grouping rule resolves this shape to exactly two groups.
        let (_, candidates) = partition_pdos(&tx_pdos);
        assert_eq!(
            group_alternatives(&candidates).len(),
            2,
            "expected 2 groups"
        );

        let device = Device {
            struct_ident: field_ident("multi_alt").expect("ident"),
            const_ident: field_ident("MULTI_ALT").expect("ident"),
            identity: Identity {
                vendor_id: 0x0000_0002,
                product_code: 0x0001_0000,
                revision: 0x0000_0001,
            },
            name: Some("MultiAlt"),
            tx_pdos: &tx_pdos,
            rx_pdos: &[],
        };

        let err = EthercrabBackend
            .emit_device(&device)
            .expect_err("two Tx alternative groups must be rejected");
        match err {
            CodegenError::MultipleAlternativeGroups { device, direction } => {
                assert_eq!(device, "multi_alt");
                assert_eq!(direction, "Tx");
            }
            other => panic!("expected MultipleAlternativeGroups, got {other:?}"),
        }
    }

    /// `<Exclude>` connected components refine grouping: PDOs that exclude each
    /// other are merged even across the union-find seed, and the merge is
    /// transitive.
    #[test]
    fn exclude_edges_merge_into_one_component() {
        // No shared Sm (each None → singleton seed), but A excludes B and B
        // excludes C → all three transitively in one group.
        let pdos = vec![
            alt_pdo(0x1A00, "A", None, false, false, vec![0x1A01]),
            alt_pdo(0x1A01, "B", None, false, false, vec![0x1A02]),
            alt_pdo(0x1A02, "C", None, false, false, vec![]),
        ];
        let (_, candidates) = partition_pdos(&pdos);
        let groups = group_alternatives(&candidates);
        assert_eq!(groups.len(), 1, "exclude chain merges A-B-C");
        assert_eq!(groups[0].len(), 3);
    }

    // -----------------------------------------------------------------------
    // Classification (refined T9): a candidate group of size 1 is NOT an
    // alternative — it is an always-on PDO whose mapping happens to be
    // reconfigurable. Only groups of >= 2 candidates are genuine alternatives.
    // -----------------------------------------------------------------------

    /// A single non-fixed/non-mandatory candidate (the only one in its Sm group)
    /// is reclassified as always-on, not a 1-variant alternative. No genuine
    /// alternative group survives.
    #[test]
    fn singleton_candidate_is_reclassified_as_always_on() {
        let candidates = [alt_pdo(0x1A00, "Solo", Some(3), false, false, vec![])];
        let refs: Vec<&Pdo> = candidates.iter().collect();
        let (singletons, genuine) = classify_alternatives(&refs);
        assert_eq!(
            singletons.iter().map(|p| p.index).collect::<Vec<_>>(),
            vec![0x1A00],
            "the lone candidate is reclassified as always-on"
        );
        assert!(genuine.is_empty(), "no genuine (>=2) alternative group");
    }

    /// Two candidates on the SAME Sm (the ALT shape) stay a genuine alternative
    /// group; nothing is reclassified as always-on.
    #[test]
    fn shared_sm_pair_stays_a_genuine_alternative() {
        let candidates = [
            alt_pdo(0x1A00, "Standard", Some(3), false, false, vec![]),
            alt_pdo(0x1A01, "Compact", Some(3), false, false, vec![]),
        ];
        let refs: Vec<&Pdo> = candidates.iter().collect();
        let (singletons, genuine) = classify_alternatives(&refs);
        assert!(singletons.is_empty(), "no singleton reclassified");
        assert_eq!(genuine.len(), 1, "one genuine alternative group");
        assert_eq!(genuine[0].len(), 2);
    }

    /// The EL1262 shape: two candidates on DISTINCT Sm (3 and 4), no Exclude →
    /// two singleton groups → BOTH reclassified as always-on, NO genuine
    /// alternative group. Declaration order is preserved.
    #[test]
    fn distinct_sm_singletons_are_both_always_on() {
        let candidates = [
            alt_pdo(0x1A00, "ChA", Some(3), false, false, vec![]),
            alt_pdo(0x1A01, "ChB", Some(4), false, false, vec![]),
        ];
        let refs: Vec<&Pdo> = candidates.iter().collect();
        let (singletons, genuine) = classify_alternatives(&refs);
        assert_eq!(
            singletons.iter().map(|p| p.index).collect::<Vec<_>>(),
            vec![0x1A00, 0x1A01],
            "both lone candidates reclassified as always-on, in declaration order"
        );
        assert!(genuine.is_empty(), "no genuine alternative group");
    }

    /// Two genuine (>=2) groups in one direction survive classification as two
    /// genuine groups (still rejected downstream by `MultipleAlternativeGroups`).
    #[test]
    fn two_multi_pdo_groups_both_genuine() {
        let candidates = [
            alt_pdo(0x1A00, "A0", Some(3), false, false, vec![]),
            alt_pdo(0x1A01, "A1", Some(3), false, false, vec![]),
            alt_pdo(0x1A10, "B0", Some(4), false, false, vec![]),
            alt_pdo(0x1A11, "B1", Some(4), false, false, vec![]),
        ];
        let refs: Vec<&Pdo> = candidates.iter().collect();
        let (singletons, genuine) = classify_alternatives(&refs);
        assert!(singletons.is_empty(), "nothing reclassified");
        assert_eq!(genuine.len(), 2, "two genuine alternative groups");
    }

    /// A mix: one genuine pair (Sm 3) plus one lone candidate (Sm 4). The lone
    /// candidate becomes always-on; the pair stays a genuine group.
    #[test]
    fn mixed_singleton_and_genuine_group() {
        let candidates = [
            alt_pdo(0x1A00, "Pair0", Some(3), false, false, vec![]),
            alt_pdo(0x1A01, "Pair1", Some(3), false, false, vec![]),
            alt_pdo(0x1A10, "Solo", Some(4), false, false, vec![]),
        ];
        let refs: Vec<&Pdo> = candidates.iter().collect();
        let (singletons, genuine) = classify_alternatives(&refs);
        assert_eq!(
            singletons.iter().map(|p| p.index).collect::<Vec<_>>(),
            vec![0x1A10]
        );
        assert_eq!(genuine.len(), 1);
        assert_eq!(genuine[0].len(), 2);
    }
}
