//! `TEST_0400` — parse a representative real Beckhoff EL3001 ESI.
//!
//! Skipped automatically unless the vendor file is present (it is not
//! redistributed with the crate). Drop `Beckhoff_EL3001.xml` into
//! `tests/fixtures/vendor/` per that directory's PROVENANCE.md to enable it.
use std::path::Path;

use taktora_ethercat_esi::parse;

#[test]
fn parses_real_beckhoff_el3001_if_present() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/vendor/Beckhoff_EL3001.xml");
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
    let file = parse(&xml).expect("real Beckhoff EL3001 ESI parses");
    assert_eq!(file.vendor.id, 0x0000_0002, "Beckhoff vendor id");
    let el3001 = file
        .devices
        .iter()
        .find(|d| d.name.as_deref().is_some_and(|n| n.contains("EL3001")))
        .expect("file contains an EL3001 device");
    assert!(!el3001.tx_pdos.is_empty(), "EL3001 has TxPDOs");
    assert!(el3001.mailbox.is_some(), "EL3001 has a CoE mailbox");
}
