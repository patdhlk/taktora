//! Test-only crate. See `tests/` for integration tests against
//! `taktora-executor-tracing` that need dev-dependencies on other sibling
//! workspace crates (notably `taktora-log`, to assert that observer events
//! bridge through the `tracing` -> `log` -> `taktora-log` path).
//!
//! This crate is `publish = false` so its dev-deps never affect the published
//! `taktora-executor-tracing` manifest — keeping `taktora-log` out of that
//! manifest, where release-plz's topological publish sort would mis-order it
//! (see issue #27 / release-plz#2697).
