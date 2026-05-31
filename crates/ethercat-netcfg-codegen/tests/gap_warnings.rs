//! `TEST_0846` — unmapped bit ranges within a device's process image are
//! non-fatal WARNINGS (`REQ_0837`): `warnings` reports them, `generate` still
//! returns `Ok`.

use ethercat_netcfg::PdoDirection;
use ethercat_netcfg_codegen::{Warning, generate, warnings};

const HEADER: &str = r"
schema_version: 1
bus: { cycle_time_ms: 2, distributed_clocks: false, max_subdevices: 16, max_pdi_bytes: 256 }
";

fn config(yaml: &str) -> ethercat_netcfg::NetworkConfig {
    ethercat_netcfg::parse(yaml).expect("fixture parses")
}

#[test]
fn interior_gap_warns_but_does_not_fail() {
    // tx entries at [0,8) and [16,24) → interior gap at bits 8..16.
    let yaml = format!(
        "{HEADER}devices:\n  - {{ label: coupler, pdos: {{ tx: [ {{ index: 0x6000, bit_offset: 0, bit_length: 8 }}, {{ index: 0x6001, bit_offset: 16, bit_length: 8 }} ] }} }}\nchannels: []\n"
    );
    let cfg = config(&yaml);

    assert!(generate(&cfg).is_ok(), "gaps are non-fatal");

    assert_eq!(
        warnings(&cfg),
        vec![Warning::UnmappedGap {
            device: "coupler".to_owned(),
            direction: PdoDirection::Tx,
            start_bit: 8,
            end_bit: 16,
        }],
    );
}

#[test]
fn fully_covered_device_has_no_warnings() {
    // tx entries at [0,8) and [8,16) → no gap.
    let yaml = format!(
        "{HEADER}devices:\n  - {{ label: coupler, pdos: {{ tx: [ {{ index: 0x6000, bit_offset: 0, bit_length: 8 }}, {{ index: 0x6001, bit_offset: 8, bit_length: 8 }} ] }} }}\nchannels: []\n"
    );
    let cfg = config(&yaml);

    assert!(
        warnings(&cfg).is_empty(),
        "fully-covered device has no gaps"
    );
}
