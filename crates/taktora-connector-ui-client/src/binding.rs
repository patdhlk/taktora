//! Contract-hash validation and the read-only fallback decision (REQ_0876).
//!
//! A client is built against an **expected** contract hash — the structural hash
//! of the [`Manifest`] the client was
//! generated / coded against. On connect (and on every epoch-triggered rebind,
//! REQ_0882) the client compares that expected hash to the live manifest's
//! [`contract_hash`](taktora_connector_ui_contract::Manifest::contract_hash):
//!
//! * **match** → a normal read-write bind ([`BindMode::ReadWrite`]); commands are
//!   enabled.
//! * **mismatch** → a degraded read-only inspect bind ([`BindMode::ReadOnly`]);
//!   ViewModels are still displayed best-effort (the client matches fields by
//!   name), but **all commands are disabled**.
//!
//! The read-only fallback is a *compatibility* control, not a security boundary:
//! it keeps a stale UI from issuing commands whose parameter layout it can no
//! longer be sure of, while still letting an operator read the live state.

use taktora_connector_ui_contract::Manifest;

/// The binding mode a client operates in after validating the contract hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindMode {
    /// The expected and live contract hashes matched: full read-write access,
    /// commands enabled.
    ReadWrite,
    /// The hashes differed: read-only inspect mode, all commands disabled
    /// (REQ_0876).
    ReadOnly,
}

impl BindMode {
    /// Whether commands may be issued in this mode.
    ///
    /// `true` only for [`BindMode::ReadWrite`]; read-only mode disables every
    /// command (REQ_0876).
    #[must_use]
    pub const fn commands_enabled(self) -> bool {
        matches!(self, BindMode::ReadWrite)
    }
}

/// Decide the [`BindMode`] from the client's `expected` contract hash and the
/// live `actual` manifest hash.
///
/// Pure, total, and case-sensitive (the contract hash is lowercase hex by
/// construction — see `taktora_connector_ui_contract::contract_hash`). Equal
/// hashes → [`BindMode::ReadWrite`]; anything else → [`BindMode::ReadOnly`].
#[must_use]
pub fn decide_bind_mode(expected: &str, actual: &str) -> BindMode {
    if expected == actual {
        BindMode::ReadWrite
    } else {
        BindMode::ReadOnly
    }
}

/// Decide the [`BindMode`] for a freshly-read `manifest` against the client's
/// `expected` hash. Convenience over [`decide_bind_mode`].
#[must_use]
pub fn bind_mode_for(expected: &str, manifest: &Manifest) -> BindMode {
    decide_bind_mode(expected, &manifest.contract_hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_hash_yields_read_write_with_commands_enabled() {
        let mode = decide_bind_mode("abc123", "abc123");
        assert_eq!(mode, BindMode::ReadWrite);
        assert!(mode.commands_enabled());
    }

    #[test]
    fn mismatched_hash_yields_read_only_with_commands_disabled() {
        let mode = decide_bind_mode("abc123", "deadbeef");
        assert_eq!(mode, BindMode::ReadOnly);
        assert!(!mode.commands_enabled());
    }

    #[test]
    fn comparison_is_case_sensitive() {
        // The contract hash is lowercase hex; an upper/lower difference is a
        // genuine mismatch, not an accidental match.
        assert_eq!(decide_bind_mode("ABCD", "abcd"), BindMode::ReadOnly);
    }

    #[test]
    fn bind_mode_for_reads_the_manifest_hash() {
        let manifest = Manifest {
            instance: "app".into(),
            epoch: 1,
            contract_hash: "feedface".into(),
            view_models: vec![],
            commands: vec![],
        };
        assert_eq!(bind_mode_for("feedface", &manifest), BindMode::ReadWrite);
        assert_eq!(bind_mode_for("other", &manifest), BindMode::ReadOnly);
    }
}
