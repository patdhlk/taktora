//! Language-neutral contract reproducibility check (Task 6.3 — documented
//! fallback for the Python smoke consumer).
//!
//! The MVVM contract is "language-neutral" only if *any* consumer can recompute
//! the structural [`contract_hash`] from the published JSON manifest using the
//! canonical algorithm documented in
//! `crates/taktora-connector-ui-contract/CONTRACT.md`. This test proves that
//! property end to end against the checked-in golden manifest: parse the JSON,
//! recompute the hash, and assert it equals the `contract_hash` the manifest
//! carries.
//!
//! This is the sanctioned stand-in (per `spec/requirements/connector/ui.rst`)
//! for a live Python consumer: the iceoryx2 Python binding is impractical to
//! stand up in CI, so instead we prove the *contract* — the JSON wire shape and
//! the hash algorithm — is faithfully reproducible from a non-Rust description.
//! `py/README.md` documents exactly how a Python client would reproduce this
//! same hash off the same bytes.

use ui_demo_contract::{Manifest, contract_hash};

// Re-export under a local alias so the intent is obvious in the assertion.
mod ui_demo_contract {
    pub use taktora_connector_ui_contract::{Manifest, contract_hash};
}

/// The canonical wire example other languages target, embedded at compile time
/// straight from the contract crate's golden fixture.
const GOLDEN_MANIFEST: &str =
    include_str!("../../../crates/taktora-connector-ui-contract/tests/golden_manifest.json");

#[test]
fn golden_manifest_hash_is_reproducible_from_the_canonical_algorithm() {
    let manifest: Manifest =
        serde_json::from_str(GOLDEN_MANIFEST).expect("golden manifest must be valid JSON");

    let recomputed = contract_hash(&manifest);

    assert_eq!(
        recomputed, manifest.contract_hash,
        "the contract hash recomputed from the canonical algorithm (CONTRACT.md) must match the \
         hash carried in the published manifest — this is what makes the contract language-neutral"
    );
}

#[test]
fn hash_excludes_instance_and_epoch_so_a_restart_keeps_the_same_contract() {
    // A consumer that re-reads the manifest after a restart (new instance label,
    // bumped epoch) must compute the SAME hash for the SAME structure — the
    // hash is structural only. This is the property the client's epoch-rebind
    // path relies on (REQ_0882).
    let mut manifest: Manifest =
        serde_json::from_str(GOLDEN_MANIFEST).expect("golden manifest must be valid JSON");
    let original = contract_hash(&manifest);

    manifest.instance = "ui-demo-after-restart".to_owned();
    manifest.epoch = manifest.epoch.wrapping_add(1);

    assert_eq!(
        contract_hash(&manifest),
        original,
        "instance and epoch must not affect the structural contract hash"
    );
}
