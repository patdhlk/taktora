//! Server-side MVVM UI connector for the taktora-connector framework
//! (FEAT_0092).
//!
//! This crate is the *authoring layer*: the routing type and (in later slices)
//! the [`ViewModel`]/[`CommandParams`] traits an application implements, plus
//! the POD building blocks the generated image types are built from.
//!
//! The non-RT publisher pump, the seqlock cell, the command handler, and the
//! `Connector` impl land in later slices.
//!
//! [`ViewModel`]: viewmodel::ViewModel
//! [`CommandParams`]: command::CommandParams

pub mod routing;

/// The language-neutral MVVM contract (manifest, schema, ack, hash).
pub use taktora_connector_ui_contract as contract;

pub use routing::UiRouting;

// Re-export the authoring derive macros so an application only depends on this
// crate. A trait and a derive macro may share a name (different namespaces),
// mirroring `serde::Serialize`.
pub use taktora_connector_ui_derive::{CommandParams, ImageEnum, ViewModel};
