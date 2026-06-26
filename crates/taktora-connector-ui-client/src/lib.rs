//! Rust reference consumer (UI client) for the taktora MVVM UI connector
//! (`FEAT_0092`, client side).
//!
//! This crate binds a UI process to a running [`UiConnector`] server purely over
//! the published, language-neutral contract — it depends on neither the executor
//! nor the server crate, only on
//! [`taktora_connector_ui_contract`] (the wire types),
//! [`taktora_connector_transport_iox`] (the iceoryx2 envelope + raw channels),
//! and `iceoryx2` itself.
//!
//! # What it does
//!
//! * **Discovery** ([`discover`], [`Client::connect`]): scan the iceoryx2 service
//!   registry for `*.manifest` services and read each [`Manifest`]; bind one
//!   instance (REQ_0877, REQ_0872).
//! * **Hash validation + read-only fallback** ([`binding`]): compare the
//!   client's expected contract hash to the live manifest; on mismatch fall back
//!   to a read-only inspect mode with all commands disabled (REQ_0876).
//! * **Property subscribe + per-field diff + staleness** ([`property`]):
//!   subscribe a ViewModel and raise [`PropertyChange`]s only for fields that
//!   actually changed, with per-ViewModel staleness (REQ_0864, REQ_0880).
//! * **Command send + ack + retry policy** ([`command`]): invoke a command with
//!   a unique correlation id, await the [`Ack`], and retry per the
//!   idempotent / epoch rules (REQ_0865, REQ_0867, REQ_0868, REQ_0882).
//! * **Stateless restart** (REQ_0881): a fresh client recovers the current
//!   manifest + ViewModel values purely from history-depth-1 redelivery, with no
//!   handshake.
//!
//! All services are pub/sub `history_size(1)` carrying JSON inside a
//! [`ConnectorEnvelope<ENVELOPE_CAPACITY>`](taktora_connector_transport_iox::ConnectorEnvelope);
//! service names are read from the manifest (REQ_0873), never constructed by
//! convention — except the bootstrap `"<instance>.manifest"` name used to find
//! the manifest itself.
//!
//! [`UiConnector`]: https://docs.rs/taktora-connector-ui
//! [`Manifest`]: taktora_connector_ui_contract::Manifest
//! [`Ack`]: taktora_connector_ui_contract::Ack

#![warn(missing_docs)]
#![deny(unsafe_code)]

pub mod binding;
pub mod client;
pub mod command;
pub mod discovery;
pub mod error;
pub mod property;

/// The fixed envelope payload capacity every UI service uses, matching the
/// server's `UiConnector::ENVELOPE_CAPACITY`. Every channel is opened as
/// `RawChannel{Reader,Writer}<ENVELOPE_CAPACITY>`.
pub const ENVELOPE_CAPACITY: usize = 4096;

pub use binding::{BindMode, bind_mode_for, decide_bind_mode};
pub use client::{Client, ClientConfig, CommandOutcome, RetryPolicy};
pub use command::{RetryDecision, mint_correlation_id, retry_decision};
pub use discovery::{discover, discover_with, list_manifest_services, manifest_service_name};
pub use error::ClientError;
pub use property::{PropertyChange, Staleness, ViewModelState, diff_fields, staleness};

// Re-export the contract so a UI binds one crate.
pub use taktora_connector_ui_contract as contract;
