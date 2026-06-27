//! Authoring derive macros for the taktora UI connector (`FEAT_0092`).
//!
//! This is the first proc-macro crate in the workspace. It turns authored
//! POD Rust types into the machinery the UI connector needs:
//!
//! * [`macro@ViewModel`] — generates an integer-lowered, `#[repr(C)]` image
//!   type (C-like enum fields lowered to their backing integer, so a torn
//!   seqlock read is always a valid bit pattern — never an invalid enum
//!   discriminant), the `to_image`/`from_image` round-trip, the
//!   [`ViewModelSchema`] builder, and the JSON encoder used off the RT path.
//! * [`macro@CommandParams`] — generates the parameter schema and captures the
//!   `#[command(idempotent)]` flag for a command's request struct.
//! * [`macro@ImageEnum`] — generates the backing-integer lowering for a C-like
//!   enum so it can be used as a `ViewModel` / `CommandParams` field.
//!
//! The generated code references the runtime crate by path
//! (`taktora_connector_ui::…`), so authoring crates only need a dependency on
//! `taktora-connector-ui` (which re-exports these macros).
//!
//! [`ViewModelSchema`]: https://docs.rs/taktora-connector-ui-contract

#![warn(missing_docs)]
#![deny(unsafe_code)]

use proc_macro::TokenStream;

mod command;
mod image_enum;
mod layout;
mod viewmodel;

/// Derive the `ViewModel` trait for a POD struct.
///
/// See the crate docs and `taktora_connector_ui::ViewModel`. Rejects non-POD
/// field types (`Vec`, `String`, `HashMap`, `i128`, `u128`) at compile time with
/// a `compile_error!`.
///
/// Any field whose type is not a scalar, a fixed array, or a `BoundedString` is
/// treated as a **C-like enum** implementing `ImageEnum`. **Nested POD structs
/// are not yet supported** (deferred from REQ_0858): a nested-struct field is
/// classified as an enum and fails to compile with the `ImageEnum`
/// `#[diagnostic::on_unimplemented]` message rather than a `compile_error!`.
///
/// The generated image type is named `{Ident}Image` (e.g. `Foo` → `FooImage`),
/// so a user type named `FooImage` in the same module would collide.
///
/// Generic, lifetime, and const-generic structs are rejected, as are
/// schema-desyncing `#[serde(rename = "...")]` / `#[serde(rename_all = "...")]`
/// attributes on the container or any field (the manifest schema is derived from
/// the Rust idents, so a rename would silently desync it from the wire).
#[proc_macro_derive(ViewModel)]
pub fn derive_view_model(input: TokenStream) -> TokenStream {
    viewmodel::derive(input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Derive the `CommandParams` trait for a command's parameter struct.
///
/// `#[command(idempotent)]` on the struct marks the command safe to auto-retry
/// under the same correlation id.
///
/// Generic, lifetime, and const-generic structs are rejected, as are
/// schema-desyncing `#[serde(rename = "...")]` / `#[serde(rename_all = "...")]`
/// attributes on the container or any field.
#[proc_macro_derive(CommandParams, attributes(command))]
pub fn derive_command_params(input: TokenStream) -> TokenStream {
    command::derive(input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Derive the `ImageEnum` trait for a C-like (field-less) enum so it can be a
/// `ViewModel` / `CommandParams` field.
///
/// The enum must carry an explicit integer `#[repr(...)]` (e.g. `#[repr(u8)]`).
///
/// Discriminants must fit in `i64`. A `#[repr(u64)]` enum with a discriminant
/// above `i64::MAX` is rejected (the lowering tracks discriminants as `i64`).
#[proc_macro_derive(ImageEnum)]
pub fn derive_image_enum(input: TokenStream) -> TokenStream {
    image_enum::derive(input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
