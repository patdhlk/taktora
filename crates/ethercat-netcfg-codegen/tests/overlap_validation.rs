//! `TEST_0844` — build-time validation rejects channels whose bit ranges
//! overlap on the same device and direction, unless one of the overlapping
//! channels opts in via `allow_overlap: true`.

use ethercat_netcfg_codegen::{ValidationError, generate};

/// A coupler with a 16-bit tx image, so two 8-bit slices both fit; whether
/// they overlap is then the only fault under test.
const DEVICES: &str = r"
schema_version: 1
bus: { cycle_time_ms: 2, distributed_clocks: false, max_subdevices: 16, max_pdi_bytes: 256 }
devices:
  - { label: din, pdos: { tx: [{ index: 0x6000, bit_offset: 0, bit_length: 16 }] } }
";

fn config(yaml: &str) -> ethercat_netcfg::NetworkConfig {
    ethercat_netcfg::parse(yaml).expect("fixture parses")
}

#[test]
fn overlapping_slices_on_same_device_and_direction_are_rejected() {
    // Two tx channels both covering bits [0, 8) → overlap.
    let yaml = format!(
        "{DEVICES}channels:\n  - {{ name: a, device: din, direction: tx, bit_offset: 0, bit_length: 8, element_type: u8 }}\n  - {{ name: b, device: din, direction: tx, bit_offset: 4, bit_length: 8, element_type: u8 }}\n"
    );
    let err = generate(&config(&yaml)).expect_err("overlapping slices must be rejected");
    assert!(
        matches!(
            err,
            ethercat_netcfg_codegen::CodegenError::Validation(
                ValidationError::OverlappingSlices { .. }
            )
        ),
        "expected OverlappingSlices, got {err:?}"
    );
}

#[test]
fn overlap_is_permitted_when_one_channel_opts_in() {
    let yaml = format!(
        "{DEVICES}channels:\n  - {{ name: a, device: din, direction: tx, bit_offset: 0, bit_length: 8, element_type: u8 }}\n  - {{ name: b, device: din, direction: tx, bit_offset: 4, bit_length: 8, element_type: u8, allow_overlap: true }}\n"
    );
    assert!(
        generate(&config(&yaml)).is_ok(),
        "overlap with allow_overlap should be permitted"
    );
}
