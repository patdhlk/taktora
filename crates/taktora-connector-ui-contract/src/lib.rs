//! taktora-connector-ui-contract — the language-neutral MVVM contract for the
//! taktora UI connector (FEAT_0092); it implements the FEAT_0095
//! manifest/schema/discovery cluster.
//!
//! This crate is pure data + serde + a structural hash: no framework deps, no
//! iceoryx2, no async. Its JSON serialization *is* the cross-language wire
//! contract (REQ_0873, REQ_0874, REQ_0875) that any UI process — Rust, Python,
//! or otherwise — binds against dynamically via the published manifest.

#![warn(missing_docs)]
#![deny(unsafe_code)]

pub mod ack;
pub mod field;
pub mod hash;
pub mod kind;
pub mod schema;
pub mod wire;

pub use ack::{Ack, RejectedCode};
pub use field::{FieldSchema, FieldType};
pub use hash::{contract_hash, validate_name};
pub use kind::Kind;
pub use schema::{CommandSchema, Manifest, ViewModelSchema};
pub use wire::{ENVELOPE_CAPACITY, MANIFEST_SERVICE_SUFFIX, manifest_service_name};
