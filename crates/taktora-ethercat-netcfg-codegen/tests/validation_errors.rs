//! `TEST_0845` — build-time validation rejects invalid network configs.
//!
//! Each sub-case exhibits exactly one fault and asserts that
//! [`taktora_ethercat_netcfg_codegen::validate`] (and therefore `generate`) returns
//! the specific matching [`ValidationError`] variant. A valid baseline
//! config returns `Ok`.

use taktora_ethercat_netcfg_codegen::{ValidationError, validate};

/// A valid baseline device set with a well-defined inline process image:
/// a coupler with `tx[0/8]` (input) + `rx[0/8]` (output), so the image
/// size for each direction is 8 bits.
const DEVICES: &str = r"
schema_version: 1
bus: { cycle_time_ms: 2, distributed_clocks: false, max_subdevices: 16, max_pdi_bytes: 256 }
devices:
  - { label: coupler, pdos: { tx: [{ index: 0x6000, bit_offset: 0, bit_length: 8 }], rx: [{ index: 0x7000, bit_offset: 0, bit_length: 8 }] } }
";

/// Parse `yaml` into a config (parsing never validates) and run [`validate`].
fn validate_yaml(yaml: &str) -> Result<(), taktora_ethercat_netcfg_codegen::CodegenError> {
    let cfg = taktora_ethercat_netcfg::parse(yaml).expect("fixture parses");
    validate(&cfg)
}

#[test]
fn baseline_valid_config_passes() {
    let yaml = format!(
        "{DEVICES}channels:\n  - {{ name: a, device: coupler, direction: tx, bit_offset: 0, bit_length: 8, element_type: u8 }}\n"
    );
    assert!(validate_yaml(&yaml).is_ok(), "baseline should validate");
}

#[test]
fn rule1_zero_length_slice() {
    let yaml = format!(
        "{DEVICES}channels:\n  - {{ name: a, device: coupler, direction: tx, bit_offset: 0, bit_length: 0, element_type: u8 }}\n"
    );
    let err = validate_yaml(&yaml).expect_err("zero-length slice must be rejected");
    assert!(
        matches!(
            err,
            taktora_ethercat_netcfg_codegen::CodegenError::Validation(
                ValidationError::ZeroLengthSlice { .. }
            )
        ),
        "expected ZeroLengthSlice, got {err:?}"
    );
}

#[test]
fn rule2_unknown_device() {
    let yaml = format!(
        "{DEVICES}channels:\n  - {{ name: a, device: ghost, direction: tx, bit_offset: 0, bit_length: 8, element_type: u8 }}\n"
    );
    let err = validate_yaml(&yaml).expect_err("unknown device must be rejected");
    assert!(
        matches!(
            err,
            taktora_ethercat_netcfg_codegen::CodegenError::Validation(
                ValidationError::UnknownDevice { .. }
            )
        ),
        "expected UnknownDevice, got {err:?}"
    );
}

#[test]
fn rule3_duplicate_channel_name() {
    let yaml = format!(
        "{DEVICES}channels:\n  - {{ name: a, device: coupler, direction: tx, bit_offset: 0, bit_length: 8, element_type: u8 }}\n  - {{ name: a, device: coupler, direction: rx, bit_offset: 0, bit_length: 8, element_type: u8 }}\n"
    );
    let err = validate_yaml(&yaml).expect_err("duplicate channel name must be rejected");
    assert!(
        matches!(
            err,
            taktora_ethercat_netcfg_codegen::CodegenError::Validation(
                ValidationError::DuplicateChannelName { .. }
            )
        ),
        "expected DuplicateChannelName, got {err:?}"
    );
}

#[test]
fn rule4_duplicate_address() {
    // Two devices pinned to the same configured address via override.
    let yaml = r"
schema_version: 1
bus: { cycle_time_ms: 2, distributed_clocks: false, max_subdevices: 16, max_pdi_bytes: 256 }
devices:
  - { label: din,  address: 0x1005, pdos: { tx: [{ index: 0x6000, bit_offset: 0, bit_length: 8 }] } }
  - { label: dout, address: 0x1005, pdos: { rx: [{ index: 0x7000, bit_offset: 0, bit_length: 8 }] } }
channels: []
";
    let err = validate_yaml(yaml).expect_err("duplicate configured address must be rejected");
    assert!(
        matches!(
            err,
            taktora_ethercat_netcfg_codegen::CodegenError::Validation(
                ValidationError::DuplicateAddress { .. }
            )
        ),
        "expected DuplicateAddress, got {err:?}"
    );
}

#[test]
fn rule5_slice_out_of_image() {
    // Image is 8 bits for tx; a 16-bit slice runs off the end.
    let yaml = format!(
        "{DEVICES}channels:\n  - {{ name: a, device: coupler, direction: tx, bit_offset: 0, bit_length: 16, element_type: u16 }}\n"
    );
    let err = validate_yaml(&yaml).expect_err("slice out of image must be rejected");
    assert!(
        matches!(
            err,
            taktora_ethercat_netcfg_codegen::CodegenError::Validation(
                ValidationError::SliceOutOfImage { .. }
            )
        ),
        "expected SliceOutOfImage, got {err:?}"
    );
}
