//! `TEST_0867` — parse a real WAGO 750-354 modular-coupler ESI (MDP).
//!
//! Skipped automatically unless the vendor file is present (it is not
//! redistributed with the crate). Drop `WAGO_750-354.xml` into
//! `tests/fixtures/vendor/` per that directory's PROVENANCE.md to enable it.
use std::path::Path;

use taktora_ethercat_esi::parse;

#[test]
fn parses_real_wago_750_354_if_present() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/vendor/WAGO_750-354.xml");
    let xml = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) if !path.exists() => {
            eprintln!("skipping: {} not present", path.display());
            return;
        }
        Err(e) => panic!(
            "vendor file present but unreadable: {} — {e}",
            path.display()
        ),
    };
    let file = parse(&xml).expect("real WAGO 750-354 ESI parses");
    assert_eq!(file.vendor.id, 0x0000_0021, "WAGO vendor id");
    let coupler = file
        .devices
        .iter()
        .find(|d| {
            d.name
                .as_deref()
                .or(d.product_type.as_deref())
                .is_some_and(|n| n.contains("750-354"))
        })
        .expect("file contains a 750-354 coupler device");
    // The coupler is an MDP modular device: it must declare slots, and the
    // file must carry a module catalog the slots reference.
    let slots = coupler
        .slots
        .as_ref()
        .expect("750-354 coupler declares <Slots>");
    assert!(!slots.slots.is_empty(), "coupler has at least one slot");
    assert!(!file.modules.is_empty(), "file carries a module catalog");
}
