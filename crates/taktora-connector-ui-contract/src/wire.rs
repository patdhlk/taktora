//! Shared wire-level constants the server and every client MUST agree on.
//!
//! These are not part of the JSON contract itself, but they govern how that
//! JSON is carried on the wire. If the server and a client disagree on any of
//! them they open *different* iceoryx2 payload types (or look for the manifest
//! under a different name) and all IPC silently fails — so they live here, in
//! the one crate both sides depend on, with a single definition each.

/// The fixed envelope payload capacity used for every UI service — the iceoryx2
/// const-generic `N` the server publishes with and every client MUST open its
/// channels with.
///
/// A ViewModel's JSON, a command's params / ack JSON, and the whole manifest
/// JSON must each fit within it. Server and client open
/// `ConnectorEnvelope<ENVELOPE_CAPACITY>`; if this value diverged between them
/// they would open incompatible payload types and all IPC would silently fail.
/// This is the single source of truth both sides alias.
pub const ENVELOPE_CAPACITY: usize = 4096;

/// The well-known suffix of a per-instance manifest service.
///
/// The bootstrap manifest service is the one service name derived by convention
/// (every other name is read from the manifest). Server and client MUST agree
/// on it, so it is defined once here and consumed by both via
/// [`manifest_service_name`].
pub const MANIFEST_SERVICE_SUFFIX: &str = ".manifest";

/// The manifest service name carrying the manifest for `instance` — the one
/// bootstrap convention shared by the server (publisher) and every client
/// (discovery).
#[must_use]
pub fn manifest_service_name(instance: &str) -> String {
    format!("{instance}{MANIFEST_SERVICE_SUFFIX}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_service_name_appends_the_shared_suffix() {
        assert_eq!(manifest_service_name("app"), "app.manifest");
    }
}
