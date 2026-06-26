//! The `#[derive(ImageEnum)]` implementation.
//!
//! Lowers a C-like (field-less) enum carrying an explicit integer `#[repr(...)]`
//! onto its backing integer: `to_repr` is `self as <repr>`, and `from_repr`
//! reconstructs by discriminant, falling back to the first variant for any
//! out-of-range value (so reconstruction is total and torn-read-safe).

use proc_macro2::{Literal, Span, TokenStream};
use quote::quote;
use syn::{Data, DeriveInput, Expr, Fields, Lit, UnOp};

use crate::layout;

struct ReprInfo {
    ident: syn::Ident,
    width: u8,
    signed: bool,
}

pub fn derive(input: TokenStream) -> syn::Result<TokenStream> {
    let ast: DeriveInput = syn::parse2(input)?;
    let ident = &ast.ident;
    let name_str = ident.to_string();
    let krate = layout::krate();

    let Data::Enum(data) = &ast.data else {
        return Err(syn::Error::new_spanned(
            ident,
            "ImageEnum can only be derived for enums",
        ));
    };

    let repr = parse_repr(&ast)?;
    let repr_ident = &repr.ident;

    let mut variants: Vec<(syn::Ident, i64)> = Vec::new();
    let mut next: i64 = 0;
    for v in &data.variants {
        if !matches!(v.fields, Fields::Unit) {
            return Err(syn::Error::new_spanned(
                v,
                "ImageEnum requires a C-like enum: variants may not carry fields",
            ));
        }
        let disc = if let Some((_, expr)) = &v.discriminant {
            eval_disc(expr)?
        } else {
            next
        };
        next = disc + 1;
        variants.push((v.ident.clone(), disc));
    }

    if variants.is_empty() {
        return Err(syn::Error::new_spanned(
            ident,
            "ImageEnum requires at least one variant",
        ));
    }

    let first = &variants[0].0;

    // VARIANTS const entries.
    let variant_entries = variants.iter().map(|(vid, disc)| {
        let vname = vid.to_string();
        quote!((#vname, #disc))
    });

    // from_repr match arms with typed literal patterns.
    let arms = variants.iter().map(|(vid, disc)| {
        let lit = typed_literal(&repr, *disc);
        quote!(#lit => Self::#vid,)
    });

    // MAX_JSON: longest variant name, JSON-quoted (+2). Names are ASCII idents,
    // so no escaping expansion applies.
    let max_name = variants
        .iter()
        .map(|(vid, _)| vid.to_string().len())
        .max()
        .unwrap_or(0);
    let max_json = max_name + 2;

    let width = repr.width;

    let expanded = quote! {
        impl #krate::ImageEnum for #ident {
            type Repr = #repr_ident;

            const VARIANTS: &'static [(&'static str, i64)] = &[ #( #variant_entries, )* ];

            const WIDTH: u8 = #width;

            const MAX_JSON: usize = #max_json;

            fn type_name() -> &'static str {
                #name_str
            }

            fn to_repr(self) -> Self::Repr {
                self as #repr_ident
            }

            fn from_repr(repr: Self::Repr) -> Self {
                match repr {
                    #( #arms )*
                    _ => Self::#first,
                }
            }
        }
    };

    Ok(expanded)
}

/// Parse the integer `#[repr(...)]` of the enum.
fn parse_repr(ast: &DeriveInput) -> syn::Result<ReprInfo> {
    for attr in &ast.attrs {
        if !attr.path().is_ident("repr") {
            continue;
        }
        let mut found: Option<ReprInfo> = None;
        attr.parse_nested_meta(|meta| {
            if let Some(id) = meta.path.get_ident() {
                let s = id.to_string();
                if let Some((width, signed)) = repr_kind(&s) {
                    found = Some(ReprInfo {
                        ident: id.clone(),
                        width,
                        signed,
                    });
                }
            }
            Ok(())
        })?;
        if let Some(info) = found {
            return Ok(info);
        }
    }
    Err(syn::Error::new_spanned(
        &ast.ident,
        "ImageEnum requires an explicit integer `#[repr(...)]`, e.g. `#[repr(u8)]`",
    ))
}

fn repr_kind(s: &str) -> Option<(u8, bool)> {
    Some(match s {
        "u8" => (1, false),
        "u16" => (2, false),
        "u32" => (4, false),
        "u64" => (8, false),
        "i8" => (1, true),
        "i16" => (2, true),
        "i32" => (4, true),
        "i64" => (8, true),
        _ => return None,
    })
}

/// Evaluate a discriminant expression (integer literal, optionally negated).
fn eval_disc(expr: &Expr) -> syn::Result<i64> {
    match expr {
        Expr::Lit(el) => {
            if let Lit::Int(li) = &el.lit {
                return li.base10_parse::<i64>();
            }
        }
        Expr::Unary(u) => {
            if let UnOp::Neg(_) = u.op {
                if let Expr::Lit(el) = &*u.expr {
                    if let Lit::Int(li) = &el.lit {
                        return Ok(-li.base10_parse::<i64>()?);
                    }
                }
            }
        }
        _ => {}
    }
    Err(syn::Error::new_spanned(
        expr,
        "ImageEnum discriminants must be plain integer literals",
    ))
}

/// Build a `repr`-typed integer literal for a discriminant value.
fn typed_literal(repr: &ReprInfo, value: i64) -> Literal {
    let _ = Span::call_site();
    if repr.signed {
        match repr.width {
            1 => Literal::i8_suffixed(value as i8),
            2 => Literal::i16_suffixed(value as i16),
            4 => Literal::i32_suffixed(value as i32),
            _ => Literal::i64_suffixed(value),
        }
    } else {
        match repr.width {
            1 => Literal::u8_suffixed(value as u8),
            2 => Literal::u16_suffixed(value as u16),
            4 => Literal::u32_suffixed(value as u32),
            _ => Literal::u64_suffixed(value as u64),
        }
    }
}
