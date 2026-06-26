//! Field-type lowering shared by the `ViewModel` and `CommandParams` derives.
//!
//! Maps each authored Rust field type onto the closed POD `FieldType` set, its
//! `#[repr(C)]` image representation, and a conservative JSON-size budget. Any
//! type outside the closed set (`Vec`, `String`, `HashMap`, `i128`, `u128`) is
//! rejected with a `compile_error!` (`REQ_0858`/`REQ_0859`/`REQ_0878`).

use proc_macro2::TokenStream;
use quote::quote;
use syn::spanned::Spanned;
use syn::{Expr, GenericArgument, Lit, PathArguments, Type};

/// The runtime crate path the generated code references.
pub fn krate() -> TokenStream {
    quote!(::taktora_connector_ui)
}

/// The error message emitted for a schema-desyncing serde attribute.
const SERDE_RENAME_MSG: &str = "`#[serde(rename/rename_all)]` is not supported on ViewModel/command types: it desyncs the manifest schema from the wire";

/// Reject schema-desyncing serde attributes (`rename` / `rename_all`).
///
/// The manifest schema field names and the JSON budget are derived from the Rust
/// idents, but serialization honors `#[serde(rename = "...")]` /
/// `#[serde(rename_all = "...")]`, which would silently desync the manifest from
/// the wire. Call this on the container's attributes and on each field's
/// attributes; it emits a spanned error pointing at the offending key.
pub fn reject_serde_rename(attrs: &[syn::Attribute]) -> syn::Result<()> {
    for attr in attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        let mut bad: Option<proc_macro2::Span> = None;
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("rename") || meta.path.is_ident("rename_all") {
                bad = Some(meta.path.span());
            }
            // Consume any `= value` or `(...)` group so the list parser can
            // advance past serde keys we don't care about.
            if meta.input.peek(syn::Token![=]) {
                let _: Expr = meta.value()?.parse()?;
            } else if meta.input.peek(syn::token::Paren) {
                meta.parse_nested_meta(|_| Ok(()))?;
            }
            Ok(())
        })?;
        if let Some(span) = bad {
            return Err(syn::Error::new(span, SERDE_RENAME_MSG));
        }
    }
    Ok(())
}

/// A classified field type.
pub enum FieldKind {
    /// A scalar (`bool`, integer, float).
    Scalar(Scalar),
    /// A fixed-length array of a scalar element.
    Array { elem: Scalar, len: usize },
    /// An inline bounded UTF-8 string of `cap` bytes (`BoundedString<cap>`).
    BoundedStr { cap: usize },
    /// A C-like enum (lowered via the `ImageEnum` trait).
    Enum(Type),
}

/// A POD scalar leaf type.
#[derive(Clone, Copy)]
pub struct Scalar {
    /// The Rust type keyword (e.g. `"f64"`).
    pub ident: &'static str,
    /// The contract `FieldType` variant name (e.g. `"F64"`).
    pub field_type: &'static str,
    /// A conservative JSON-encoded byte budget for one value.
    pub json_budget: usize,
}

const SCALARS: &[Scalar] = &[
    Scalar {
        ident: "bool",
        field_type: "Bool",
        json_budget: 5,
    },
    Scalar {
        ident: "i8",
        field_type: "I8",
        json_budget: 4,
    },
    Scalar {
        ident: "i16",
        field_type: "I16",
        json_budget: 6,
    },
    Scalar {
        ident: "i32",
        field_type: "I32",
        json_budget: 11,
    },
    Scalar {
        ident: "i64",
        field_type: "I64",
        json_budget: 20,
    },
    Scalar {
        ident: "u8",
        field_type: "U8",
        json_budget: 3,
    },
    Scalar {
        ident: "u16",
        field_type: "U16",
        json_budget: 5,
    },
    Scalar {
        ident: "u32",
        field_type: "U32",
        json_budget: 10,
    },
    Scalar {
        ident: "u64",
        field_type: "U64",
        json_budget: 20,
    },
    Scalar {
        ident: "f32",
        field_type: "F32",
        json_budget: 16,
    },
    Scalar {
        ident: "f64",
        field_type: "F64",
        json_budget: 24,
    },
];

/// Types rejected with an explicit message (`REQ_0858`).
const REJECTED: &[(&str, &str)] = &[
    (
        "String",
        "use `taktora_connector_ui::BoundedString<CAP>` for an inline bounded string",
    ),
    (
        "Vec",
        "dynamically-sized `Vec` is not POD; use a fixed-length array `[T; N]`",
    ),
    (
        "HashMap",
        "`HashMap` is not POD; UI ViewModels are fixed-layout",
    ),
    (
        "BTreeMap",
        "`BTreeMap` is not POD; UI ViewModels are fixed-layout",
    ),
    (
        "i128",
        "128-bit integers are outside the closed POD field set",
    ),
    (
        "u128",
        "128-bit integers are outside the closed POD field set",
    ),
];

fn scalar_by_ident(ident: &str) -> Option<Scalar> {
    SCALARS.iter().copied().find(|s| s.ident == ident)
}

/// The last path segment's identifier, if `ty` is a type path.
fn last_segment_ident(ty: &Type) -> Option<String> {
    if let Type::Path(tp) = ty {
        tp.path.segments.last().map(|s| s.ident.to_string())
    } else {
        None
    }
}

fn lit_usize(expr: &Expr) -> Option<usize> {
    if let Expr::Lit(el) = expr {
        if let Lit::Int(li) = &el.lit {
            return li.base10_parse::<usize>().ok();
        }
    }
    None
}

/// Classify a field type, or return a spanned `compile_error!`-bearing error.
pub fn classify(ty: &Type) -> syn::Result<FieldKind> {
    // Fixed-length array of a scalar.
    if let Type::Array(arr) = ty {
        let elem = scalar_by_ident(&last_segment_ident(&arr.elem).unwrap_or_default()).ok_or_else(
            || {
                syn::Error::new(
                    arr.elem.span(),
                    "ViewModel array elements must be a POD scalar (bool/int/float)",
                )
            },
        )?;
        let len = lit_usize(&arr.len).ok_or_else(|| {
            syn::Error::new(arr.len.span(), "array length must be an integer literal")
        })?;
        return Ok(FieldKind::Array { elem, len });
    }

    let ident = last_segment_ident(ty)
        .ok_or_else(|| syn::Error::new(ty.span(), "unsupported ViewModel field type"))?;

    if let Some((_, why)) = REJECTED.iter().find(|(name, _)| *name == ident) {
        return Err(syn::Error::new(
            ty.span(),
            format!("`{ident}` is not a valid ViewModel field type: {why}"),
        ));
    }

    if let Some(scalar) = scalar_by_ident(&ident) {
        return Ok(FieldKind::Scalar(scalar));
    }

    if ident == "BoundedString" {
        let cap = bounded_string_cap(ty)?;
        return Ok(FieldKind::BoundedStr { cap });
    }

    // Anything else (any non-scalar / non-array / non-`BoundedString` type) is
    // treated as a C-like enum implementing `ImageEnum`. Nested POD structs are
    // *not yet supported* (deferred from REQ_0858): a nested-struct field lands
    // here and fails to compile via `ImageEnum`'s `#[diagnostic::on_unimplemented]`
    // message rather than a `compile_error!`.
    Ok(FieldKind::Enum(ty.clone()))
}

/// Extract the `CAP` const-generic argument of a `BoundedString<CAP>`.
fn bounded_string_cap(ty: &Type) -> syn::Result<usize> {
    if let Type::Path(tp) = ty {
        if let Some(seg) = tp.path.segments.last() {
            if let PathArguments::AngleBracketed(args) = &seg.arguments {
                for arg in &args.args {
                    match arg {
                        GenericArgument::Const(expr) => {
                            if let Some(cap) = lit_usize(expr) {
                                return Ok(cap);
                            }
                        }
                        // Some parses surface a bare const as a type path.
                        GenericArgument::Type(Type::Path(p)) => {
                            if let Some(id) = p.path.get_ident() {
                                if let Ok(cap) = id.to_string().parse::<usize>() {
                                    return Ok(cap);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    Err(syn::Error::new(
        ty.span(),
        "BoundedString needs a literal capacity, e.g. `BoundedString<16>`",
    ))
}

/// The image-struct field type for a classified field.
pub fn image_field_type(kind: &FieldKind, original: &Type) -> TokenStream {
    let krate = krate();
    match kind {
        FieldKind::Scalar(_) | FieldKind::Array { .. } | FieldKind::BoundedStr { .. } => {
            quote!(#original)
        }
        FieldKind::Enum(ty) => quote!(<#ty as #krate::ImageEnum>::Repr),
    }
}

/// The expression lowering `self.#field` into the image.
pub fn to_image_expr(kind: &FieldKind, field: &TokenStream) -> TokenStream {
    let krate = krate();
    if let FieldKind::Enum(_) = kind {
        quote!(#krate::ImageEnum::to_repr(self.#field))
    } else {
        quote!(self.#field)
    }
}

/// The expression reconstructing the field from `image.#field`.
pub fn from_image_expr(kind: &FieldKind, field: &TokenStream) -> TokenStream {
    let krate = krate();
    if let FieldKind::Enum(ty) = kind {
        quote!(<#ty as #krate::ImageEnum>::from_repr(image.#field))
    } else {
        quote!(image.#field)
    }
}

/// The `FieldType` descriptor expression for the schema.
pub fn field_type_expr(kind: &FieldKind) -> TokenStream {
    let krate = krate();
    let ft = quote!(#krate::contract::FieldType);
    match kind {
        FieldKind::Scalar(s) => {
            let variant = syn::Ident::new(s.field_type, proc_macro2::Span::call_site());
            quote!(#ft::#variant)
        }
        FieldKind::Array { elem, len } => {
            let variant = syn::Ident::new(elem.field_type, proc_macro2::Span::call_site());
            let len = u32::try_from(*len).unwrap_or(u32::MAX);
            quote!(#ft::Array { elem: ::std::boxed::Box::new(#ft::#variant), len: #len })
        }
        FieldKind::BoundedStr { cap } => {
            let cap = u32::try_from(*cap).unwrap_or(u32::MAX);
            quote!(#ft::Str { cap: #cap })
        }
        FieldKind::Enum(ty) => quote!(<#ty as #krate::ImageEnum>::field_type()),
    }
}

/// The JSON byte budget term for one field (name overhead + value budget).
///
/// Overhead is `"name":` (name + 3) plus a separating comma (1). The value
/// budget is a literal for everything but enums, which defer to the enum's
/// `ImageEnum::MAX_JSON` associated const so the sum stays a valid `const` expr.
pub fn json_budget_term(kind: &FieldKind, name: &str) -> TokenStream {
    let krate = krate();
    let overhead = name.len() + 4;
    match kind {
        FieldKind::Scalar(s) => {
            let total = overhead + s.json_budget;
            quote!(#total)
        }
        FieldKind::Array { elem, len } => {
            // brackets (2) + len * (value + comma)
            let value = 2 + len * (elem.json_budget + 1);
            let total = overhead + value;
            quote!(#total)
        }
        FieldKind::BoundedStr { cap } => {
            // worst-case every byte escaped to `\uXXXX` (6) + 2 quotes
            let value = cap * 6 + 2;
            let total = overhead + value;
            quote!(#total)
        }
        FieldKind::Enum(ty) => {
            quote!(#overhead + <#ty as #krate::ImageEnum>::MAX_JSON)
        }
    }
}
