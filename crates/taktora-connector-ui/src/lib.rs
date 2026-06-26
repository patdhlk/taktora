//! Server-side MVVM UI connector for the taktora-connector framework
//! (`FEAT_0092`).
//!
//! This crate is the *authoring layer*: the [`ViewModel`] and [`CommandParams`]
//! traits an application implements (usually via the re-exported derives), the
//! [`UiRouting`] routing type, and the POD building blocks the generated image
//! types are built from ([`BoundedString`], [`ImageEnum`]).
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

#![warn(missing_docs)]
#![deny(unsafe_code)]

pub mod bounded_string;
pub mod command;
pub mod connector;
pub mod health;
pub mod hot_scalar;
pub mod iox_publisher;
pub mod manifest;
pub mod options;
pub mod property;
pub mod pump;
pub mod routing;
pub mod system;
pub mod viewmodel;

// The seqlock cell is an internal implementation detail of `Property`; it is
// crate-private so the only way to drive the producer side is the move-only
// `Property` handle (single-writer invariant, type-enforced).
mod cell;

/// The language-neutral MVVM contract (manifest, schema, ack, hash).
pub use taktora_connector_ui_contract as contract;

pub use bounded_string::BoundedString;
pub use command::{
    CanExecute, CommandHandler, CommandHandlerHandle, CommandInvocation, CommandParams,
    CommandTransport, CorrelationId, IoxCommandTransport, MockCommandTransport, RegisteredCommand,
    can_execute_entry, command_channel,
};
pub use connector::UiConnector;
pub use health::PublishHealth;
pub use hot_scalar::{HotScalar, HotScalarValue};
pub use iox_publisher::IoxVmPublisher;
pub use manifest::{ManifestBuilder, manifest_entry};
pub use options::{UiConnectorOptions, UiConnectorOptionsBuilder};
pub use property::{Property, PropertyReader};
pub use pump::{
    EncodeFn, MockPublisher, Pump, PumpEntry, PumpHandle, PumpTickStats, VmPublisher,
    property_entry,
};
pub use routing::UiRouting;
pub use system::SystemViewModel;
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
