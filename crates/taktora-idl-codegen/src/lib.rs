//! Policy-owning codegen layer for the message-plane IR (`FEAT_0110`).
//!
//! This crate is the message-plane twin of the device-plane `esi-codegen`. It
//! owns the naming policy ([`naming`], `REQ_0938`), defines the
//! [`MessageBackend`] trait, and exposes [`generate`]. It resolves a
//! [`taktora_idl_core::Module`] into a borrowing [`ResolvedModule`] — every
//! identifier already chosen, every field classified — and hands that to a
//! backend, which emits [`proc_macro2::TokenStream`] (`REQ_0939`). The crate is
//! **plane-generic**: it knows nothing about CAN, CDR, or any wire format. A
//! backend (e.g. `taktora-idl-codegen-can`) owns that.

use proc_macro2::{Ident, TokenStream};
use quote::quote;
use taktora_idl_core::{Module, Scalar, Type};

pub mod naming;

/// How a struct field is laid out in Rust and on the wire — enough for a
/// backend to emit (de)serialization without re-deriving anything.
#[derive(Debug, Clone)]
pub enum FieldKind {
    /// An unsigned integer scalar.
    Unsigned(Scalar),
    /// A signed integer scalar.
    Signed(Scalar),
    /// An IEEE-754 float scalar.
    Float(Scalar),
    /// A value-table enum, carried on `underlying`.
    Enum {
        /// The generated enum type identifier.
        ident: Ident,
        /// The integer scalar the discriminant rides on.
        underlying: Scalar,
    },
}

/// A struct field with naming and classification already resolved.
#[derive(Debug, Clone)]
pub struct ResolvedField<'a> {
    /// `snake_case` field identifier.
    pub ident: Ident,
    /// Original field name, for a backend to join against its own layout data.
    pub source_name: &'a str,
    /// Rust type tokens for the field declaration (`u16`, `EngineDataGear`, …).
    pub rust_ty: TokenStream,
    /// On-wire classification.
    pub kind: FieldKind,
}

/// A struct (message) with naming resolved.
#[derive(Debug, Clone)]
pub struct ResolvedStruct<'a> {
    /// `PascalCase` type identifier.
    pub ident: Ident,
    /// Original struct name, for backend layout lookup.
    pub source_name: &'a str,
    /// Resolved fields, in declaration order.
    pub fields: Vec<ResolvedField<'a>>,
}

/// An enum (value table) with naming resolved.
#[derive(Debug, Clone)]
pub struct ResolvedEnum<'a> {
    /// `PascalCase` type identifier.
    pub ident: Ident,
    /// Original enum name.
    pub source_name: &'a str,
    /// The integer scalar the discriminant rides on.
    pub underlying: Scalar,
    /// Variants as `(identifier, discriminant)` pairs.
    pub variants: Vec<(Ident, i64)>,
}

/// A whole module with all naming resolved.
#[derive(Debug, Clone)]
pub struct ResolvedModule<'a> {
    /// Module name (source).
    pub name: &'a str,
    /// Resolved enums.
    pub enums: Vec<ResolvedEnum<'a>>,
    /// Resolved structs.
    pub structs: Vec<ResolvedStruct<'a>>,
}

/// Errors raised while resolving or generating.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CodegenError {
    /// A field uses an IR construct this slice does not yet emit (nested
    /// struct, array, sequence, or bounded string). DBC never produces these;
    /// other frontends will, and lifting the restriction is a follow-on.
    #[error("`{struct_name}.{field}` uses unsupported field shape: {detail}")]
    UnsupportedField {
        /// Owning struct.
        struct_name: String,
        /// Field name.
        field: String,
        /// What was unsupported.
        detail: &'static str,
    },
    /// A field references an enum that the module does not define.
    #[error("`{struct_name}.{field}` references undefined enum `{enum_name}`")]
    UnknownEnum {
        /// Owning struct.
        struct_name: String,
        /// Field name.
        field: String,
        /// The dangling enum name.
        enum_name: String,
    },
    /// A backend rejected an otherwise-resolved construct (e.g. an unsupported
    /// byte order or a float on a path that cannot carry one).
    #[error("backend rejected `{what}`: {detail}")]
    Backend {
        /// What was rejected.
        what: String,
        /// Why.
        detail: String,
    },
    /// Two distinct source names resolve to the same Rust type identifier.
    /// IR validation only dedups verbatim source names, so collisions the
    /// naming policy introduces (e.g. `Foo_Bar` and `FooBar`) surface here.
    #[error("two types resolve to the same Rust identifier `{ident}`")]
    DuplicateType {
        /// The colliding identifier.
        ident: String,
    },
    /// Two fields of one struct resolve to the same Rust identifier.
    #[error("struct `{struct_name}` has two fields resolving to `{ident}`")]
    DuplicateField {
        /// Owning struct (source name).
        struct_name: String,
        /// The colliding identifier.
        ident: String,
    },
    /// Two variants of one enum resolve to the same Rust identifier.
    #[error("enum `{enum_name}` has two variants resolving to `{ident}`")]
    DuplicateVariant {
        /// Owning enum (source name).
        enum_name: String,
        /// The colliding identifier.
        ident: String,
    },
}

/// A backend that turns a [`ResolvedModule`]'s items into Rust token streams.
///
/// Implementors are policy-free: identifiers and field classification are
/// already decided (`REQ_0938`). A backend supplies the wire format — for CAN,
/// the [`WireType`](https://docs.rs/taktora-idl-wire) implementation.
pub trait MessageBackend {
    /// Items emitted once at the top of the generated module (typically a
    /// `use` of the runtime support crate).
    fn preamble(&self) -> TokenStream;

    /// Emit the definition and any trait impls for one enum.
    ///
    /// # Errors
    ///
    /// [`CodegenError::Backend`] if the backend cannot represent the enum.
    fn emit_enum(&self, resolved: &ResolvedEnum) -> Result<TokenStream, CodegenError>;

    /// Emit the struct definition and its serialization impl.
    ///
    /// # Errors
    ///
    /// [`CodegenError::Backend`] if the backend cannot represent the struct
    /// (e.g. it has no layout for it, or a field's byte order is unsupported).
    fn emit_struct(&self, resolved: &ResolvedStruct) -> Result<TokenStream, CodegenError>;
}

/// Resolve `module` and emit a token stream of items via `backend`.
///
/// The returned stream is unformatted (`REQ_0939`); the build layer renders it
/// with `prettyplease`. It does not wrap items in a `mod`; the caller chooses
/// the module boundary.
///
/// # Errors
///
/// Any [`CodegenError`] from resolution or from the backend.
pub fn generate(
    module: &Module,
    backend: &dyn MessageBackend,
) -> Result<TokenStream, CodegenError> {
    let resolved = resolve(module)?;
    let mut out = backend.preamble();
    for e in &resolved.enums {
        out.extend(backend.emit_enum(e)?);
    }
    for s in &resolved.structs {
        out.extend(backend.emit_struct(s)?);
    }
    Ok(out)
}

/// Apply naming and field classification to produce a [`ResolvedModule`].
///
/// # Errors
///
/// [`CodegenError::UnsupportedField`] or [`CodegenError::UnknownEnum`].
pub fn resolve(module: &Module) -> Result<ResolvedModule<'_>, CodegenError> {
    let enums: Vec<ResolvedEnum> = module
        .enums
        .iter()
        .map(|e| ResolvedEnum {
            ident: naming::type_ident(&e.name),
            source_name: &e.name,
            underlying: e.underlying,
            variants: e
                .variants
                .iter()
                .map(|v| (naming::variant_ident(&v.name), v.value))
                .collect(),
        })
        .collect();

    let structs = module
        .structs
        .iter()
        .map(|s| resolve_struct(s, &enums))
        .collect::<Result<Vec<_>, _>>()?;

    check_unique_idents(&enums, &structs)?;

    Ok(ResolvedModule {
        name: &module.name,
        enums,
        structs,
    })
}

/// Reject identifier collisions the naming policy can introduce between source
/// names that `idl-core` considered distinct. Without this, two such names emit
/// duplicate Rust items and the generated module fails to compile.
fn check_unique_idents(
    enums: &[ResolvedEnum],
    structs: &[ResolvedStruct],
) -> Result<(), CodegenError> {
    let mut type_idents = Vec::with_capacity(enums.len() + structs.len());
    for ident in enums
        .iter()
        .map(|e| &e.ident)
        .chain(structs.iter().map(|s| &s.ident))
    {
        let name = ident.to_string();
        if type_idents.contains(&name) {
            return Err(CodegenError::DuplicateType { ident: name });
        }
        type_idents.push(name);
    }

    for e in enums {
        let mut seen = Vec::with_capacity(e.variants.len());
        for (ident, _) in &e.variants {
            let name = ident.to_string();
            if seen.contains(&name) {
                return Err(CodegenError::DuplicateVariant {
                    enum_name: e.source_name.to_owned(),
                    ident: name,
                });
            }
            seen.push(name);
        }
    }

    for s in structs {
        let mut seen = Vec::with_capacity(s.fields.len());
        for f in &s.fields {
            let name = f.ident.to_string();
            if seen.contains(&name) {
                return Err(CodegenError::DuplicateField {
                    struct_name: s.source_name.to_owned(),
                    ident: name,
                });
            }
            seen.push(name);
        }
    }

    Ok(())
}

fn resolve_struct<'a>(
    s: &'a taktora_idl_core::Struct,
    enums: &[ResolvedEnum<'a>],
) -> Result<ResolvedStruct<'a>, CodegenError> {
    let fields = s
        .fields
        .iter()
        .map(|f| resolve_field(&s.name, f, enums))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ResolvedStruct {
        ident: naming::type_ident(&s.name),
        source_name: &s.name,
        fields,
    })
}

fn resolve_field<'a>(
    struct_name: &str,
    f: &'a taktora_idl_core::Field,
    enums: &[ResolvedEnum<'a>],
) -> Result<ResolvedField<'a>, CodegenError> {
    let (rust_ty, kind) = match &f.ty {
        Type::Scalar(s) => (scalar_tokens(*s), scalar_kind(*s)),
        Type::Enum(name) => {
            let resolved = enums
                .iter()
                .find(|e| e.source_name == name.as_str())
                .ok_or_else(|| CodegenError::UnknownEnum {
                    struct_name: struct_name.to_owned(),
                    field: f.name.clone(),
                    enum_name: name.as_str().to_owned(),
                })?;
            let ident = &resolved.ident;
            (
                quote!(#ident),
                FieldKind::Enum {
                    ident: resolved.ident.clone(),
                    underlying: resolved.underlying,
                },
            )
        }
        Type::String { .. } => return Err(unsupported(struct_name, f, "bounded string")),
        Type::Array { .. } => return Err(unsupported(struct_name, f, "array")),
        Type::Sequence { .. } => return Err(unsupported(struct_name, f, "sequence")),
        Type::Struct(_) => return Err(unsupported(struct_name, f, "nested struct")),
    };
    Ok(ResolvedField {
        ident: naming::field_ident(&f.name),
        source_name: &f.name,
        rust_ty,
        kind,
    })
}

fn unsupported(
    struct_name: &str,
    f: &taktora_idl_core::Field,
    detail: &'static str,
) -> CodegenError {
    CodegenError::UnsupportedField {
        struct_name: struct_name.to_owned(),
        field: f.name.clone(),
        detail,
    }
}

const fn scalar_kind(s: Scalar) -> FieldKind {
    if s.is_float() {
        FieldKind::Float(s)
    } else if s.is_signed_integer() {
        FieldKind::Signed(s)
    } else {
        FieldKind::Unsigned(s)
    }
}

/// Rust primitive tokens for a scalar.
#[must_use]
pub fn scalar_tokens(s: Scalar) -> TokenStream {
    match s {
        Scalar::Bool => quote!(bool),
        Scalar::U8 => quote!(u8),
        Scalar::U16 => quote!(u16),
        Scalar::U32 => quote!(u32),
        Scalar::U64 => quote!(u64),
        Scalar::I8 => quote!(i8),
        Scalar::I16 => quote!(i16),
        Scalar::I32 => quote!(i32),
        Scalar::I64 => quote!(i64),
        Scalar::F32 => quote!(f32),
        Scalar::F64 => quote!(f64),
    }
}

#[cfg(test)]
mod tests {
    use super::{CodegenError, resolve};
    use taktora_idl_core::{EnumDef, EnumVariant, Field, Module, Scalar, Struct, Type};

    fn struct_with(name: &str, fields: Vec<Field>) -> Struct {
        Struct {
            name: name.to_owned(),
            fields,
        }
    }

    #[test]
    fn colliding_type_idents_are_rejected() {
        // `Foo_Bar` and `FooBar` both pascalize to `FooBar`.
        let module = Module {
            structs: vec![
                struct_with("Foo_Bar", vec![]),
                struct_with("FooBar", vec![]),
            ],
            ..Module::new("m")
        };
        assert!(matches!(
            resolve(&module),
            Err(CodegenError::DuplicateType { ident }) if ident == "FooBar"
        ));
    }

    #[test]
    fn colliding_field_idents_are_rejected() {
        // `CoolantTemp` and `coolant_temp` both snake to `coolant_temp`.
        let module = Module {
            structs: vec![struct_with(
                "Frame",
                vec![
                    Field::new("CoolantTemp", Type::scalar(Scalar::U8)),
                    Field::new("coolant_temp", Type::scalar(Scalar::U8)),
                ],
            )],
            ..Module::new("m")
        };
        assert!(matches!(
            resolve(&module),
            Err(CodegenError::DuplicateField { ident, .. }) if ident == "coolant_temp"
        ));
    }

    #[test]
    fn colliding_variant_idents_are_rejected() {
        let module = Module {
            enums: vec![EnumDef {
                name: "E".to_owned(),
                underlying: Scalar::U8,
                variants: vec![
                    EnumVariant {
                        name: "Ab".to_owned(),
                        value: 0,
                    },
                    EnumVariant {
                        name: "ab".to_owned(),
                        value: 1,
                    },
                ],
            }],
            ..Module::new("m")
        };
        assert!(matches!(
            resolve(&module),
            Err(CodegenError::DuplicateVariant { ident, .. }) if ident == "Ab"
        ));
    }
}
