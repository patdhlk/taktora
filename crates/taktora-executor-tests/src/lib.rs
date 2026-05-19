//! Test-only crate. See `tests/` for integration tests against
//! `taktora-executor` that need dev-dependencies on other sibling
//! workspace crates (notably `taktora-bounded-alloc` for the
//! zero-allocation hot-path tests).
//!
//! This crate is `publish = false` so its dev-deps never affect the
//! published `taktora-executor` manifest.
