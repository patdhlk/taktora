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

use proc_macro2::TokenStream;
use quote::quote;
use taktora_ethercat_esi::Pdo;
use taktora_ethercat_esi_codegen::{CodegenBackend, CodegenError, Device, Identity, field_ident};

mod typemap;

use typemap::{Layout, ScalarKind};

/// The code-emitting backend producing `EsiDevice` impls for the rt contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct EthercrabBackend;

/// One resolved input/output field: its struct field ident, Rust type, and the
/// bit layout used to build the read/write expression.
struct ResolvedField {
    ident: proc_macro2::Ident,
    rust_type: TokenStream,
    layout: Layout,
}

/// Resolve every non-padding entry of a PDO list into [`ResolvedField`]s,
/// running the bit-offset accumulator over declaration order. Padding entries
/// (`index == 0`) advance the offset but emit no field. Returns the resolved
/// fields and the total bit width of the PDO group.
fn resolve_fields(pdos: &[Pdo]) -> Result<(Vec<ResolvedField>, usize), CodegenError> {
    let mut fields = Vec::new();
    let mut offset = 0usize;

    for pdo in pdos {
        for entry in &pdo.entries {
            // Padding / gap entry: advance the running offset, emit no field.
            if entry.index == 0 {
                offset += entry.bit_length as usize;
                continue;
            }

            let raw_name = entry.name.clone().unwrap_or_else(|| {
                // Unnamed non-padding entry: entry_<index:04x>_<sub>.
                format!("entry_{:04x}_{}", entry.index, entry.sub_index)
            });
            let ident = field_ident(&raw_name)?;

            let (field_type, layout) = typemap::resolve(
                entry.data_type.as_ref(),
                entry.bit_length,
                offset,
                entry.index,
                entry.sub_index,
                &raw_name,
            )?;

            offset += layout.width();
            fields.push(ResolvedField {
                ident,
                rust_type: field_type.rust_type,
                layout,
            });
        }
    }

    Ok((fields, offset))
}

/// Build the per-field read expression assigning out of `bits` into `self`.
fn read_stmt(field: &ResolvedField) -> TokenStream {
    let ident = &field.ident;
    match field.layout {
        Layout::Bool { offset } => quote! { self.#ident = bits[#offset]; },
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
                    quote! { self.#ident = bits[#range].load_le::<#ty>(); }
                }
                ScalarKind::F32 => {
                    quote! { self.#ident = f32::from_bits(bits[#range].load_le::<u32>()); }
                }
                ScalarKind::F64 => {
                    quote! { self.#ident = f64::from_bits(bits[#range].load_le::<u64>()); }
                }
            }
        }
    }
}

/// Build the per-field write expression storing `self` into `bits`.
fn write_stmt(field: &ResolvedField) -> TokenStream {
    let ident = &field.ident;
    match field.layout {
        Layout::Bool { offset } => quote! { bits.set(#offset, self.#ident); },
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
                    quote! { bits[#range].store_le::<#ty>(self.#ident); }
                }
                ScalarKind::F32 => {
                    quote! { bits[#range].store_le::<u32>(self.#ident.to_bits()); }
                }
                ScalarKind::F64 => {
                    quote! { bits[#range].store_le::<u64>(self.#ident.to_bits()); }
                }
            }
        }
    }
}

impl CodegenBackend for EthercrabBackend {
    fn emit_device(&self, device: &Device) -> Result<TokenStream, CodegenError> {
        let struct_ident = &device.struct_ident;
        let const_ident = &device.const_ident;

        let (inputs, input_bits) = resolve_fields(device.tx_pdos)?;
        let (outputs, output_bits) = resolve_fields(device.rx_pdos)?;

        // Struct fields: one per non-padding entry across both directions.
        // TODO(slice-2): de-dup field idents colliding across PDOs; for the
        // bullet-1 device the names are distinct, so no suffixing is needed.
        let field_defs = inputs.iter().chain(outputs.iter()).map(|f| {
            let ident = &f.ident;
            let ty = &f.rust_type;
            quote! { pub #ident: #ty }
        });

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
        let decode_guard = length_guard(input_bits);
        let read_stmts = inputs.iter().map(read_stmt);
        let decode_import = bitfield_import(&inputs);
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
            let guard = length_guard(output_bits);
            let write_stmts = outputs.iter().map(write_stmt);
            let import = bitfield_import(&outputs);
            quote! {
                #import
                #guard
                #(#write_stmts)*
                Ok(())
            }
        };

        Ok(quote! {
            #[allow(non_camel_case_types)]
            #[derive(Debug, Default, Clone)]
            pub struct #struct_ident {
                #(#field_defs,)*
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

    fn emit_module_root(&self, _devices: &[Device]) -> Result<TokenStream, CodegenError> {
        // Registry emission is deferred to a later slice (REQ_0523).
        Ok(TokenStream::new())
    }
}

/// Emit the `BitField` trait import iff at least one field is a multi-bit
/// load/store (a bool-only or empty body has no `load_le`/`store_le` call and
/// must not import the trait, lest the generated code warn on an unused import).
fn bitfield_import(fields: &[ResolvedField]) -> TokenStream {
    if fields
        .iter()
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
}
