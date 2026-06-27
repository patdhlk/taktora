//! Server-side MVVM UI connector for the taktora-connector framework
//! (`FEAT_0092`).
//!
//! This is the **server side** of the UI connector: the half that runs in the
//! taktora application process and publishes ViewModels / accepts commands for a
//! UI to bind against over the language-neutral [`contract`]. It provides:
//!
//! * [`UiConnector`] — the [`Connector`](taktora_connector_host::Connector)
//!   implementation that registers with the executor.
//! * The seqlock latest-value cells ([`Property`] / [`PropertyReader`]) and the
//!   non-RT publisher [`Pump`] that drains them off the real-time path.
//! * The acceptance-ack command handler ([`CommandHandler`]) over the iceoryx2
//!   command transport.
//! * Manifest publishing ([`ManifestBuilder`]) and the mandatory
//!   [`SystemViewModel`] heartbeat.
//! * The MVVM authoring API on [`UiConnector`]:
//!   [`add_view_model`](UiConnector::add_view_model),
//!   [`add_command`](UiConnector::add_command), and
//!   [`add_hot_scalar`](UiConnector::add_hot_scalar).
//!
//! Applications implement the [`ViewModel`] and [`CommandParams`] traits (usually
//! via the re-exported derives) over POD building blocks ([`BoundedString`],
//! [`ImageEnum`]); the connector handles the wire.
//!
//! # Quickstart
//!
//! ```no_run
//! use serde::Serialize;
//! use taktora_connector_ui::{UiConnector, UiConnectorOptions, ViewModel};
//! use taktora_connector_host::Connector;
//! use taktora_executor::Executor;
//!
//! // 1. Declare a ViewModel (a fixed-layout POD struct).
//! #[derive(Clone, Debug, PartialEq, Serialize, ViewModel)]
//! struct Stepper {
//!     position: f64,
//! }
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // 2. Construct the connector and author its ViewModels *before* registering.
//! let mut connector = UiConnector::new(
//!     UiConnectorOptions::builder().instance("demo").build(),
//! )?;
//! let position = connector.add_view_model::<Stepper>("Stepper");
//!
//! // 3. Register with the executor: this spawns the pump + command handler.
//! let mut executor = Executor::builder().build()?;
//! connector.register_with(&mut executor)?;
//!
//! // 4. Drive the ViewModel from the application; the pump publishes it.
//! position.set(&Stepper { position: 1.0 });
//! # Ok(())
//! # }
//! ```
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

    /// `size_of` routed through this crate so that, when a generated image type is
    /// ill-formed (an enum field whose type does not implement
    /// [`ImageEnum`](crate::ImageEnum)), the `Sized` obligation is reported
    /// against this crate's source span rather than against `core::mem::size_of`.
    /// std-internal diagnostic spans render differently across rustc versions,
    /// which made `trybuild` fixtures drift; this crate's span is stable.
    #[must_use]
    pub const fn image_size<T>() -> usize {
        ::core::mem::size_of::<T>()
    }

    /// The image-struct field type for a C-like enum field: the enum's backing
    /// integer ([`ImageEnum::Repr`](crate::ImageEnum::Repr)), transparently.
    ///
    /// Generated `#[derive(ViewModel)]` images wrap every enum field in this
    /// newtype instead of naming `<E as ImageEnum>::Repr` directly. The two are
    /// layout-identical (`#[repr(transparent)]`), but the indirection means that
    /// when `E` does **not** implement [`ImageEnum`](crate::ImageEnum) (e.g. a
    /// nested POD struct, which is deferred), every downstream obligation
    /// (`Copy`, `Clone`, `Sized`) is reported against *this crate's* `E: ImageEnum`
    /// bound — a stable span — rather than against `core`'s `Copy`/`Clone` lang
    /// items, whose diagnostic rendering drifts between rustc versions and made
    /// the `trybuild` rejection fixtures non-portable.
    #[repr(transparent)]
    pub struct EnumImage<E: crate::ImageEnum>(pub <E as crate::ImageEnum>::Repr);

    impl<E: crate::ImageEnum> ::std::clone::Clone for EnumImage<E> {
        fn clone(&self) -> Self {
            *self
        }
    }

    impl<E: crate::ImageEnum> ::std::marker::Copy for EnumImage<E> {}
}
