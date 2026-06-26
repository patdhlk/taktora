//! taktora-connector-ui-contract — the language-neutral MVVM contract for the
//! taktora UI connector (FEAT_0092).
//!
//! This crate is pure data + serde + a structural hash: no framework deps, no
//! iceoryx2, no async. Its JSON serialization *is* the cross-language wire
//! contract (REQ_0873, REQ_0874, REQ_0875) that any UI process — Rust, Python,
//! or otherwise — binds against dynamically via the published manifest.

#![warn(missing_docs)]
#![deny(unsafe_code)]

pub mod field;
pub mod kind;

pub use field::{FieldSchema, FieldType};
pub use kind::Kind;
