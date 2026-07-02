//! `PayloadCodec` implementations for the taktora-connector framework.
//!
//! Implements [`BB_0003`](../../spec/architecture/connector.rst):
//!
//! Available codecs (the doc links resolve only when the corresponding
//! cargo feature is enabled):
//!
//! * `JsonCodec` — `serde_json`-backed codec behind the default-on
//!   `json` cargo feature (`REQ_0212`).
//! * `BinaryCodec` — `bincode`-backed fixed-width binary codec behind
//!   the opt-in `binary` cargo feature (`REQ_0212`).
//! * `MsgPackCodec` — `rmp-serde`-backed `MessagePack` codec behind the
//!   opt-in `msgpack` cargo feature (`REQ_0989`).
//!
//! The [`PayloadCodec`] trait itself is defined in
//! [`taktora_connector_core::codec`] and re-exported here for callers
//! that only want to depend on `taktora-connector-codec`.

#![warn(missing_docs)]

#[cfg(feature = "binary")]
pub mod binary;
#[cfg(feature = "json")]
pub mod json;
#[cfg(feature = "msgpack")]
pub mod msgpack;

#[cfg(feature = "binary")]
pub use binary::{BinaryCodec, Endian};
#[cfg(feature = "json")]
pub use json::JsonCodec;
#[cfg(feature = "msgpack")]
pub use msgpack::MsgPackCodec;

pub use taktora_connector_core::PayloadCodec;
