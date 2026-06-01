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
    CodegenBackend, CodegenError, Device, Identity, field_ident, pdo_field_ident, pdo_struct_ident,
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
    pdos: &[Pdo],
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

/// The `self.<…>` access path for a field: bare `self.<field>` in the flat
/// shape, or `self.<pdo>.<field>` when the direction is split into per-PDO
/// sub-structs.
fn access_path(pdo_field: Option<&Ident>, field: &Ident) -> TokenStream {
    pdo_field.map_or_else(|| quote! { self.#field }, |pdo| quote! { self.#pdo.#field })
}

/// Build the per-field read expression assigning out of `bits` into the field's
/// access path (`self.<field>` or `self.<pdo>.<field>`).
fn read_stmt(pdo_field: Option<&Ident>, field: &ResolvedField) -> TokenStream {
    let target = access_path(pdo_field, &field.ident);
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
    }
}

/// Build the per-field write expression storing the field's access path into
/// `bits`.
fn write_stmt(pdo_field: Option<&Ident>, field: &ResolvedField) -> TokenStream {
    let source = access_path(pdo_field, &field.ident);
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
    }
}

impl CodegenBackend for EthercrabBackend {
    fn emit_device(&self, device: &Device) -> Result<TokenStream, CodegenError> {
        let struct_ident = &device.struct_ident;
        let const_ident = &device.const_ident;

        let (inputs, input_bits) = resolve_direction(device.tx_pdos, struct_ident)?;
        let (outputs, output_bits) = resolve_direction(device.rx_pdos, struct_ident)?;

        // A direction with more than one PDO is split into per-PDO sub-structs
        // so that entry names repeated across channels (e.g. each EL2004
        // channel's `Output`) no longer collide; a single-PDO direction stays
        // flat, keeping the bullet-1 device byte-identical.
        let inputs_flat = inputs.len() <= 1;
        let outputs_flat = outputs.len() <= 1;

        // Sub-struct type definitions for every split direction.
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

        // Device-struct fields: flat directions contribute their entry fields
        // directly; split directions contribute one sub-struct field per PDO.
        let mut device_fields = TokenStream::new();
        device_fields.extend(direction_device_fields(&inputs, inputs_flat));
        device_fields.extend(direction_device_fields(&outputs, outputs_flat));

        let Identity {
            vendor_id,
            product_code,
            revision,
        } = device.identity;

        let input_len = input_bits.div_ceil(8);
        let output_len = output_bits.div_ceil(8);

        // The `BitField` trait is only in scope when a body actually calls
        // `load_le`/`store_le`; a bool-only or no-op body must not emit the
        // `use`, or the generated code warns on an unused import.
        let read_stmts = direction_stmts(&inputs, inputs_flat, read_stmt);
        let decode_import = bitfield_import(&inputs);
        let decode_guard = length_guard(input_bits);
        let decode_body = quote! {
            #decode_import
            #decode_guard
            #(#read_stmts)*
            Ok(())
        };

        let encode_body = if outputs.is_empty() {
            // No RxPdo: a no-op encode. Suppress the unused `bits` warning.
            quote! { let _ = bits; Ok(()) }
        } else {
            let write_stmts = direction_stmts(&outputs, outputs_flat, write_stmt);
            let import = bitfield_import(&outputs);
            let guard = length_guard(output_bits);
            quote! {
                #import
                #guard
                #(#write_stmts)*
                Ok(())
            }
        };

        Ok(quote! {
            #sub_structs

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

/// Emit one PDO sub-struct: a plain `#[derive(Debug, Default, Clone)]` data
/// holder with one `pub` field per resolved entry, no trait impls.
fn emit_sub_struct(pdo: &ResolvedPdo) -> TokenStream {
    let ident = &pdo.struct_ident;
    let field_defs = pdo.fields.iter().map(|f| {
        let fi = &f.ident;
        let ty = &f.rust_type;
        quote! { pub #fi: #ty }
    });
    quote! {
        #[allow(non_camel_case_types)]
        #[derive(Debug, Default, Clone)]
        pub struct #ident {
            #(#field_defs,)*
        }
    }
}

/// The device-struct field declarations for one direction: flat → one field per
/// entry; split → one `pub <pdo>: <SubStruct>` per PDO.
fn direction_device_fields(pdos: &[ResolvedPdo], flat: bool) -> TokenStream {
    if flat {
        let defs = pdos.iter().flat_map(|p| p.fields.iter()).map(|f| {
            let fi = &f.ident;
            let ty = &f.rust_type;
            quote! { pub #fi: #ty, }
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

/// Emit the `BitField` trait import iff at least one field across the
/// direction's PDOs is a multi-bit load/store (a bool-only or empty body has no
/// `load_le`/`store_le` call and must not import the trait, lest the generated
/// code warn on an unused import).
fn bitfield_import(pdos: &[ResolvedPdo]) -> TokenStream {
    if pdos
        .iter()
        .flat_map(|p| p.fields.iter())
        .any(|f| matches!(f.layout, Layout::Field { .. }))
    {
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
            other @ Layout::Bool { .. } => panic!("expected sub-byte Field layout, got {other:?}"),
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
            other @ Layout::Bool { .. } => panic!("expected 32-bit Field layout, got {other:?}"),
        }
    }

    #[test]
    fn type_map_untyped_width_inferred() {
        assert_eq!(ty(None, 1), "bool");
        assert_eq!(ty(None, 12), "u16");
    }

    #[test]
    fn type_map_strings_are_unsupported() {
        let err = typemap::resolve(Some(&DataType::VisibleString), 64, 0, 0x6000, 5, "name")
            .expect_err("strings unsupported");
        assert!(matches!(err, CodegenError::UnsupportedEntryType { .. }));
    }

    #[test]
    fn type_map_named_is_unsupported() {
        let dt = DataType::Named("DT2008".to_owned());
        let err =
            typemap::resolve(Some(&dt), 16, 0, 0x6000, 5, "name").expect_err("named unsupported");
        match err {
            CodegenError::UnsupportedEntryType { reason, .. } => assert_eq!(reason, "DT2008"),
            other => panic!("unexpected error: {other:?}"),
        }
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
