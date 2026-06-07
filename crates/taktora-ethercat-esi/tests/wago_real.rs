//! `TEST_0867` — parse real WAGO 750-354 coupler ESI files (MDP).
//!
//! Skipped automatically unless the vendor files are present (they are not
//! redistributed with the crate). Drop `WAGO_750-354_25.xml` (full MDP
//! catalog) and `WAGO_750_354_no_modules.xml` (module-less variant) into
//! `tests/fixtures/vendor/` per that directory's PROVENANCE.md to enable them.
use std::path::{Path, PathBuf};

use taktora_ethercat_esi::{EsiFile, parse};

/// Read a gated vendor fixture: `None` (with a skip message) when the file is
/// absent, a panic when it exists but cannot be read.
fn read_vendor_file(name: &str) -> Option<(PathBuf, String)> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/vendor/{name}"));
    match std::fs::read_to_string(&path) {
        Ok(s) => Some((path, s)),
        Err(_) if !path.exists() => {
            eprintln!("skipping: {} not present", path.display());
            None
        }
        Err(e) => panic!(
            "vendor file present but unreadable: {} — {e}",
            path.display()
        ),
    }
}

/// Find a device whose name or product type mentions the 750-354 coupler.
fn find_coupler(file: &EsiFile) -> Option<&taktora_ethercat_esi::EsiDevice> {
    file.devices.iter().find(|d| {
        d.name
            .as_deref()
            .or(d.product_type.as_deref())
            .is_some_and(|n| n.contains("750-354"))
    })
}

#[test]
fn parses_real_wago_750_354_mdp_catalog_if_present() {
    let Some((_, xml)) = read_vendor_file("WAGO_750-354_25.xml") else {
        return;
    };
    let file = parse(&xml).expect("real WAGO 750-354 ESI parses");
    assert_eq!(file.vendor.id, 0x0000_0021, "WAGO vendor id");
    let coupler = find_coupler(&file).expect("file contains a 750-354 coupler device");
    // The coupler is an MDP modular device: it must declare slots, and the
    // file must carry a module catalog the slots reference (WAGO ships a
    // single-file catalog — modules live in the same ESI as the coupler).
    let slots = coupler
        .slots
        .as_ref()
        .expect("750-354 coupler declares <Slots>");
    assert!(!slots.slots.is_empty(), "coupler has at least one slot");
    assert!(!file.modules.is_empty(), "file carries a module catalog");
    // Every slot-referenced ModuleIdent should resolve against the catalog —
    // the faithful IR keeps both sides; consumers rely on the join holding.
    let catalog: std::collections::HashSet<u32> =
        file.modules.iter().filter_map(|m| m.ident).collect();
    for slot in &slots.slots {
        for mi in &slot.module_idents {
            assert!(
                catalog.contains(&mi.ident),
                "slot references ModuleIdent {:#010x} missing from the catalog",
                mi.ident
            );
        }
    }
}

#[test]
fn parses_real_wago_750_354_no_modules_variant_if_present() {
    let Some((_, xml)) = read_vendor_file("WAGO_750_354_no_modules.xml") else {
        return;
    };
    let file = parse(&xml).expect("real WAGO 750-354 (no-modules variant) ESI parses");
    assert_eq!(file.vendor.id, 0x0000_0021, "WAGO vendor id");
    assert!(
        find_coupler(&file).is_some(),
        "file contains a 750-354 coupler device"
    );
    assert!(
        file.modules.is_empty(),
        "the no-modules variant carries no module catalog"
    );
}
