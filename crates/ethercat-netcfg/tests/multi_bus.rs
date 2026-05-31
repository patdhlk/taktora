//! `TEST_0831` — one-file-one-bus (`REQ_0822` / `ADR_0096`).
//!
//! A `network.yaml` describes exactly one bus. A YAML *stream* packing
//! more than one `---`-separated document is rejected; a single-document
//! config parses as before.

use ethercat_netcfg::{NetcfgError, parse};

const MULTI_BUS: &str = "\
schema_version: 1
bus: { cycle_time_ms: 2, distributed_clocks: false, max_subdevices: 16, max_pdi_bytes: 256 }
devices: [{ label: coupler, pdos: { tx: [{ index: 0x6000, bit_offset: 0, bit_length: 8 }] } }]
channels: []
---
schema_version: 1
bus: { cycle_time_ms: 4, distributed_clocks: false, max_subdevices: 8, max_pdi_bytes: 128 }
devices: [{ label: other, pdos: { rx: [{ index: 0x7000, bit_offset: 0, bit_length: 8 }] } }]
channels: []
";

const SINGLE_BUS: &str = "\
schema_version: 1
bus: { cycle_time_ms: 2, distributed_clocks: false, max_subdevices: 16, max_pdi_bytes: 256 }
devices: [{ label: coupler, pdos: { tx: [{ index: 0x6000, bit_offset: 0, bit_length: 8 }] } }]
channels: []
";

#[test]
fn rejects_multi_bus_stream_but_accepts_single_bus() {
    // A multi-document stream packs more than one bus into one file.
    let multi = parse(MULTI_BUS);
    assert!(
        matches!(multi, Err(NetcfgError::MultipleBuses { .. })),
        "expected MultipleBuses, got {multi:?}"
    );

    // A single-document config keeps parsing as today.
    assert!(
        parse(SINGLE_BUS).is_ok(),
        "single-bus config should parse: {:?}",
        parse(SINGLE_BUS)
    );
}
