//! The `#[derive(CommandParams)]` implementation.
//!
//! Generates a [`CommandParams`] impl: the parameter `FieldSchema` list (using
//! the shared field lowering) and the `IDEMPOTENT` flag captured from
//! `#[command(idempotent)]` (`REQ_0868`/`REQ_0873`).

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields};

use crate::layout::{self, field_type_expr};

pub fn derive(input: TokenStream) -> syn::Result<TokenStream> {
    let ast: DeriveInput = syn::parse2(input)?;
    let ident = &ast.ident;
    let krate = layout::krate();

    if !ast.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &ast.generics,
            "CommandParams does not support generic, lifetime, or const-generic structs",
        ));
    }
    layout::reject_serde_rename(&ast.attrs)?;

    let idempotent = parse_idempotent(&ast)?;

    let Data::Struct(data) = &ast.data else {
        return Err(syn::Error::new_spanned(
            ident,
            "CommandParams can only be derived for structs",
        ));
    };

    // Unit / tuple structs have no named params; only named-field and unit
    // (no params) structs are supported.
    let mut schema_fields = Vec::new();
    match &data.fields {
        Fields::Named(named) => {
            for field in &named.named {
                layout::reject_serde_rename(&field.attrs)?;
                let fident = field.ident.as_ref().expect("named field");
                let fname = fident.to_string();
                let kind = layout::classify(&field.ty)?;
                let ft = field_type_expr(&kind);
                schema_fields.push(quote! {
                    #krate::contract::FieldSchema {
                        name: #fname.to_owned(),
                        ty: #ft,
                    }
                });
            }
        }
        Fields::Unit => {}
        Fields::Unnamed(_) => {
            return Err(syn::Error::new_spanned(
                ident,
                "CommandParams requires named fields (or no fields)",
            ));
        }
    }

    let expanded = quote! {
        impl #krate::CommandParams for #ident {
            const IDEMPOTENT: bool = #idempotent;

            fn params() -> ::std::vec::Vec<#krate::contract::FieldSchema> {
                ::std::vec![ #( #schema_fields, )* ]
            }
        }
    };

    Ok(expanded)
}

/// Parse `#[command(idempotent)]` on the struct.
fn parse_idempotent(ast: &DeriveInput) -> syn::Result<bool> {
    let mut idempotent = false;
    for attr in &ast.attrs {
        if !attr.path().is_ident("command") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("idempotent") {
                idempotent = true;
                Ok(())
            } else {
                Err(meta.error("unknown `command` attribute; expected `idempotent`"))
            }
        })?;
    }
    Ok(idempotent)
}
