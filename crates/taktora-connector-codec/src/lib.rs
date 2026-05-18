//! `PayloadCodec` implementations for the taktora-connector framework.
//!
//! Implements [`BB_0003`](../../spec/architecture/connector.rst):
//!
//! * [`JsonCodec`] — `serde_json`-backed codec behind the default-on
//!   `json` cargo feature (`REQ_0212`).
//!
//! The [`PayloadCodec`] trait itself is defined in
//! [`taktora_connector_core::codec`] and re-exported here for callers
//! that only want to depend on `taktora-connector-codec`.

#![warn(missing_docs)]

#[cfg(feature = "json")]
pub mod json;

#[cfg(feature = "json")]
pub use json::JsonCodec;

pub use taktora_connector_core::PayloadCodec;
