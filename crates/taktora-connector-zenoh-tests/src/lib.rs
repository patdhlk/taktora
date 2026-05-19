//! Test-only crate. See `tests/` for integration tests against
//! `taktora-connector-zenoh` that need dev-dependencies on other
//! sibling workspace crates (notably `taktora-connector-codec`).
//!
//! This crate is `publish = false` so its dev-deps never affect the
//! published `taktora-connector-zenoh` manifest.
//!
//! The `zenoh-integration` feature is enabled unconditionally on the
//! parent `taktora-connector-zenoh` dev-dep because one moved test
//! (`tests/real_session.rs`) is gated behind it.
