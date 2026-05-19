//! Test-only crate. See `tests/` for integration tests against
//! `taktora-connector-ethercat` that need dev-dependencies on other
//! sibling workspace crates (notably `taktora-connector-codec` and
//! `taktora-bounded-alloc`).
//!
//! This crate is `publish = false` so its dev-deps never affect the
//! published `taktora-connector-ethercat` manifest.
