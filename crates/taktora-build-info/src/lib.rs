//! Compile-time build identity for taktora binaries (`BB_0123`, `REQ_0990`,
//! `ADR_0132`).
//!
//! The [`build.rs`](../build.rs) companion captures the git commit, dirty flag,
//! build timestamp, and `rustc` version at build time and threads them in as
//! `TAKTORA_BUILD_*` env constants; this module reads them back into the
//! [`CAPTURED`] constant so identity travels *with* the binary — a
//! deployed device names its exact commit with no runtime configuration.
//!
//! This crate carries **zero** dependencies and no edge to the medkit core, so
//! it is reusable by any taktora binary. A binary maps [`CAPTURED`] into the
//! `taktora_medkit_model::BuildInfo` DTO and injects it through the gateway's
//! `with_build_info` seam; the extractable core never depends on this crate
//! (`REQ_0916`, `ADR_0111`).

#![forbid(unsafe_code)]

/// Source identity of the running binary, captured at compile time.
///
/// Every field is a `&'static str` baked in by [`build.rs`](../build.rs), except
/// [`git_dirty`](Self::git_dirty). Git-derived fields are `"unknown"` when the
/// build had no `.git` available.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct BuildInfo {
    /// Full 40-hex git commit hash, or `"unknown"`.
    pub git_sha: &'static str,
    /// Abbreviated git commit hash, or `"unknown"`.
    pub git_short: &'static str,
    /// `git describe --tags --always` (nearest tag + distance, or the short
    /// hash when untagged), or `"unknown"`.
    pub git_describe: &'static str,
    /// Whether the worktree had uncommitted changes at build time.
    pub git_dirty: bool,
    /// Build time as a UTC RFC3339 instant, or `"unknown"`.
    pub build_timestamp: &'static str,
    /// `rustc --version` of the compiler that built this crate, or `"unknown"`.
    pub rustc_version: &'static str,
}

/// The identity captured for **this** build.
pub const CAPTURED: BuildInfo = BuildInfo {
    git_sha: env!("TAKTORA_BUILD_GIT_SHA"),
    git_short: env!("TAKTORA_BUILD_GIT_SHORT"),
    git_describe: env!("TAKTORA_BUILD_GIT_DESCRIBE"),
    git_dirty: dirty(),
    build_timestamp: env!("TAKTORA_BUILD_TIMESTAMP"),
    rustc_version: env!("TAKTORA_BUILD_RUSTC"),
};

/// `build.rs` emits `"1"` when the worktree was dirty, `"0"` otherwise.
const fn dirty() -> bool {
    let bytes = env!("TAKTORA_BUILD_GIT_DIRTY").as_bytes();
    bytes.len() == 1 && bytes[0] == b'1'
}

/// The identity captured for this build. Equivalent to reading [`CAPTURED`].
#[must_use]
pub const fn capture() -> BuildInfo {
    CAPTURED
}

#[cfg(test)]
mod tests {
    use super::CAPTURED;

    #[test]
    fn capture_is_populated() {
        // The env constants are always present (build.rs emits every key), so
        // no field is empty; git fields are a hash or the `"unknown"` fallback.
        assert!(!CAPTURED.git_sha.is_empty());
        assert!(!CAPTURED.git_short.is_empty());
        assert!(!CAPTURED.build_timestamp.is_empty());
        assert!(!CAPTURED.rustc_version.is_empty());
    }
}
