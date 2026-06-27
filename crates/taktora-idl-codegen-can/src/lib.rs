//! CAN/DBC backend (`REQ_0862`): emits `taktora_idl_wire::WireType` impls.
//!
//! Given the DBC layout sidecar produced by `taktora-idl-dbc`, this backend
//! emits, per message struct, a `WireType` implementation whose `encode`/
//! `decode` bit-pack each field at the start bit, length, and byte order the
//! DBC declared — calling only the primitives in `taktora-idl-wire`, never
//! serde (`REQ_0861`). Naming and field classification arrive already resolved
//! from `taktora-idl-codegen` (`REQ_0863`); this crate adds only the wire format.
//!
//! ## Scope of this slice
//!
//! Integer and enum fields are emitted. `bool`/float fields are rejected with
//! [`CodegenError::Backend`] — DBC never produces them, and admitting them is a
//! follow-on. Both DBC byte orders (Intel `@1`, Motorola `@0`) are supported,
//! since `taktora-idl-wire` handles both bit-numbering conventions.

use proc_macro2::{Literal, TokenStream};
use quote::quote;
use taktora_idl_codegen::{
    CodegenError, FieldKind, MessageBackend, ResolvedEnum, ResolvedField, ResolvedStruct,
    scalar_tokens,
};
use taktora_idl_dbc::{ByteOrder, DbcLayout, SignalLayout};

/// A [`MessageBackend`] that emits CAN-frame `WireType` impls from a layout.
pub struct CanBackend<'a> {
    layout: &'a DbcLayout,
}

impl<'a> CanBackend<'a> {
    /// Bind the backend to the DBC layout sidecar for the module being emitted.
    #[must_use]
    pub const fn new(layout: &'a DbcLayout) -> Self {
        Self { layout }
    }
}

impl MessageBackend for CanBackend<'_> {
    fn preamble(&self) -> TokenStream {
        quote! {
            use taktora_idl_wire::{
                ByteOrder, WireError, WireType,
                pack_signed, pack_unsigned, unpack_signed, unpack_unsigned,
            };
        }
    }

    fn emit_enum(&self, e: &ResolvedEnum) -> Result<TokenStream, CodegenError> {
        let ident = &e.ident;
        let repr = scalar_tokens(e.underlying);
        let variants = e.variants.iter().map(|(vident, value)| {
            let lit = Literal::i64_unsuffixed(*value);
            quote!(#vident = #lit)
        });
        let arms = e.variants.iter().map(|(vident, value)| {
            let lit = Literal::i64_unsuffixed(*value);
            quote!(#lit => ::core::result::Result::Ok(Self::#vident))
        });
        Ok(quote! {
            #[derive(Clone, Copy, Debug, PartialEq, Eq)]
            #[repr(#repr)]
            pub enum #ident {
                #(#variants),*
            }

            impl #ident {
                /// The raw discriminant transmitted on the wire.
                #[must_use]
                pub const fn to_raw(self) -> #repr {
                    self as #repr
                }

                /// Recover a variant from its raw discriminant.
                ///
                /// # Errors
                ///
                /// [`WireError::UnknownEnumValue`] if `raw` has no defined variant.
                pub fn from_raw(raw: #repr) -> ::core::result::Result<Self, WireError> {
                    match raw {
                        #(#arms,)*
                        _ => ::core::result::Result::Err(WireError::UnknownEnumValue),
                    }
                }
            }
        })
    }

    fn emit_struct(&self, s: &ResolvedStruct) -> Result<TokenStream, CodegenError> {
        let frame = self
            .layout
            .frame(s.source_name)
            .ok_or_else(|| CodegenError::Backend {
                what: s.source_name.to_owned(),
                detail: "no CAN frame layout for this struct".to_owned(),
            })?;
        let ident = &s.ident;
        let dlc = frame.dlc as usize;

        let field_decls = s.fields.iter().map(|f| {
            let fident = &f.ident;
            let ty = &f.rust_ty;
            quote!(pub #fident: #ty)
        });

        let mut encodes = Vec::with_capacity(s.fields.len());
        let mut decodes = Vec::with_capacity(s.fields.len());
        for f in &s.fields {
            let sig =
                signal_for(frame.signals.as_slice(), f).ok_or_else(|| CodegenError::Backend {
                    what: format!("{}.{}", s.source_name, f.source_name),
                    detail: "no signal layout for this field".to_owned(),
                })?;
            encodes.push(encode_field(s.source_name, f, sig)?);
            let (fident, expr) = decode_field(s.source_name, f, sig)?;
            decodes.push(quote!(#fident: #expr));
        }

        Ok(quote! {
            #[derive(Clone, Copy, Debug, PartialEq, Eq)]
            pub struct #ident {
                #(#field_decls,)*
            }

            impl WireType for #ident {
                const MAX_SERIALIZED_LEN: usize = #dlc;

                fn encode(&self, buf: &mut [u8]) -> ::core::result::Result<usize, WireError> {
                    if buf.len() < #dlc {
                        return ::core::result::Result::Err(WireError::BufferTooSmall);
                    }
                    for b in &mut buf[..#dlc] {
                        *b = 0;
                    }
                    #(#encodes)*
                    ::core::result::Result::Ok(#dlc)
                }

                fn decode(buf: &[u8]) -> ::core::result::Result<Self, WireError> {
                    if buf.len() < #dlc {
                        return ::core::result::Result::Err(WireError::BufferTooSmall);
                    }
                    ::core::result::Result::Ok(Self {
                        #(#decodes,)*
                    })
                }
            }
        })
    }
}

fn signal_for<'s>(signals: &'s [SignalLayout], f: &ResolvedField) -> Option<&'s SignalLayout> {
    signals.iter().find(|s| s.field_name == f.source_name)
}

fn order_tokens(order: ByteOrder) -> TokenStream {
    match order {
        ByteOrder::LittleEndian => quote!(ByteOrder::LittleEndian),
        ByteOrder::BigEndian => quote!(ByteOrder::BigEndian),
    }
}

fn encode_field(
    struct_name: &str,
    f: &ResolvedField,
    sig: &SignalLayout,
) -> Result<TokenStream, CodegenError> {
    let fident = &f.ident;
    let start = sig.start_bit;
    let len = sig.bit_len;
    let order = order_tokens(sig.byte_order);
    let tokens = match &f.kind {
        FieldKind::Unsigned(_) => quote! {
            pack_unsigned(buf, #start, #len, #order, self.#fident as u64)?;
        },
        FieldKind::Signed(_) => quote! {
            pack_signed(buf, #start, #len, #order, self.#fident as i64)?;
        },
        FieldKind::Enum { .. } => quote! {
            pack_unsigned(buf, #start, #len, #order, self.#fident.to_raw() as u64)?;
        },
        FieldKind::Float(_) => return Err(reject_float(struct_name, f)),
    };
    Ok(tokens)
}

fn decode_field<'a>(
    struct_name: &str,
    f: &'a ResolvedField,
    sig: &SignalLayout,
) -> Result<(&'a proc_macro2::Ident, TokenStream), CodegenError> {
    let fident = &f.ident;
    let start = sig.start_bit;
    let len = sig.bit_len;
    let order = order_tokens(sig.byte_order);
    let ty = &f.rust_ty;
    let expr = match &f.kind {
        FieldKind::Unsigned(_) => quote! {
            unpack_unsigned(buf, #start, #len, #order)? as #ty
        },
        FieldKind::Signed(_) => quote! {
            unpack_signed(buf, #start, #len, #order)? as #ty
        },
        FieldKind::Enum { ident, underlying } => {
            let under = scalar_tokens(*underlying);
            quote! {
                #ident::from_raw(unpack_unsigned(buf, #start, #len, #order)? as #under)?
            }
        }
        FieldKind::Float(_) => return Err(reject_float(struct_name, f)),
    };
    Ok((fident, expr))
}

fn reject_float(struct_name: &str, f: &ResolvedField) -> CodegenError {
    CodegenError::Backend {
        what: format!("{struct_name}.{}", f.source_name),
        detail: "CAN backend handles integer and enum fields only".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::CanBackend;
    use taktora_idl_codegen::generate;
    use taktora_idl_dbc::{lower, parse};

    const SAMPLE: &str = r#"
BO_ 256 EngineData: 8 ECU
 SG_ Rpm : 0|16@1+ (0.25,0) [0|16383.75] "rpm" Dashboard
 SG_ CoolantTemp : 16|8@1- (1,-40) [-40|215] "degC" Dashboard
 SG_ Gear : 24|4@1+ (1,0) [0|7] "" Dashboard

VAL_ 256 Gear 0 "Neutral" 1 "First" 2 "Second" ;
"#;

    #[test]
    fn generated_tokens_are_valid_rust() {
        let db = parse(SAMPLE).unwrap();
        let lowered = lower(&db, "vehicle").unwrap();
        let backend = CanBackend::new(&lowered.layout);
        let tokens = generate(&lowered.module, &backend).unwrap();

        // The strongest cheap check short of compiling: the emitted stream
        // parses as a Rust source file.
        let file: syn::File = syn::parse2(tokens).expect("generated tokens parse as Rust");
        let rendered = prettyplease::unparse(&file);

        assert!(rendered.contains("impl WireType for EngineData"));
        assert!(rendered.contains("pub enum EngineDataGear"));
        assert!(rendered.contains("coolant_temp"));
        assert!(rendered.contains("pack_signed(buf"));
    }
}
