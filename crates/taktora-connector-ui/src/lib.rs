//! Server-side MVVM UI connector for the taktora-connector framework
//! (FEAT_0092).
//!
//! This crate is the *authoring layer*: the [`ViewModel`] (and, in a later
//! slice, `CommandParams`) traits an application implements (usually via the
//! re-exported derives), the [`UiRouting`] routing type, and the POD building
//! blocks the generated image types are built from ([`ImageEnum`]).
//!
//! The non-RT publisher pump, the seqlock cell, the command handler, and the
//! `Connector` impl land in later slices; this crate currently exposes the
//! types the derive macros target.
//!
//! # Re-exports
//!
//! The language-neutral contract is re-exported as [`contract`], and the
//! authoring derives are re-exported so an application only depends on this
//! crate.

pub mod bounded_string;
pub mod routing;
pub mod viewmodel;

/// The language-neutral MVVM contract (manifest, schema, ack, hash).
pub use taktora_connector_ui_contract as contract;

pub use bounded_string::BoundedString;
pub use routing::UiRouting;
pub use viewmodel::{ImageEnum, ViewModel};

// Re-export the authoring derive macros so an application only depends on this
// crate. A trait and a derive macro may share a name (different namespaces),
// mirroring `serde::Serialize`.
pub use taktora_connector_ui_derive::{CommandParams, ImageEnum, ViewModel};

#[doc(hidden)]
pub mod __private {
    //! Implementation details referenced by generated derive code. Not a stable
    //! public API.
    pub use serde_json;
    pub use taktora_connector_ui_contract as contract;
}
