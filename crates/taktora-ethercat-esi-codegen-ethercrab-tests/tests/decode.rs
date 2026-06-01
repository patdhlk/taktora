//! End-to-end proof for the EtherCAT ESI device-codegen toolchain.
//!
//! These tests exercise the device struct produced by `build.rs` (which runs
//! `taktora-ethercat-esi-build` over `esi/el3001_like.xml`) to confirm the
//! whole spine — ESI XML → generated Rust → `EsiDevice::decode_inputs` — works
//! against a known PDI bit-pattern.

use bitvec::view::BitView;
use taktora_ethercat_esi_codegen_ethercrab_tests::generated::EL3001_like;
use taktora_ethercat_esi_rt::{BitSlice, EsiDevice, EsiError, Identity, Lsb0};

/// The identity the codegen derives from `el3001_like.xml`.
const EXPECTED_IDENTITY: Identity = Identity {
    vendor_id: 0x0000_0002,
    product_code: 0x0bb9_3052,
    revision: 0x0010_0000,
};

#[test]
fn decode_round_trip() {
    let mut dev = EL3001_like::default();

    // 3-byte PDI: bit 0 (underrange) set, value = 0x1234 little-endian in
    // bits 8..24. Lsb0 byte layout: [byte0 = 0x01, byte1 = 0x34, byte2 = 0x12].
    let bytes: [u8; 3] = [0x01, 0x34, 0x12];
    let bits: &BitSlice<u8, Lsb0> = bytes.view_bits::<Lsb0>();

    dev.decode_inputs(bits).expect("decode should succeed");

    assert!(dev.underrange, "underrange bit should decode to true");
    assert_eq!(dev.value, 0x1234, "value should decode to 0x1234");

    assert_eq!(dev.identity(), EXPECTED_IDENTITY);
    assert_eq!(dev.input_len(), 3);
    assert_eq!(dev.output_len(), 0);
}

#[test]
fn buffer_too_short() {
    let mut dev = EL3001_like::default();

    // Only 2 bytes (16 bits) supplied; the device needs 24 bits.
    let bytes: [u8; 2] = [0x01, 0x34];
    let bits: &BitSlice<u8, Lsb0> = bytes.view_bits::<Lsb0>();

    let err = dev
        .decode_inputs(bits)
        .expect_err("decode should fail on a short buffer");

    match err {
        EsiError::BufferTooShort {
            expected_bits,
            got_bits,
        } => {
            assert_eq!(expected_bits, 24);
            assert_eq!(got_bits, 16);
        }
    }
}

#[test]
fn object_safety_smoke() {
    let dev = EL3001_like::default();
    let _: &dyn EsiDevice = &dev;
}

/// The generated module must match the committed golden snapshot byte-for-byte
/// (modulo trailing whitespace), so codegen drift is caught in review.
#[test]
fn golden_snapshot_matches() {
    let generated = include_str!(concat!(env!("OUT_DIR"), "/devices.rs"));
    let golden = include_str!("golden/devices.rs");

    assert_eq!(
        normalize(generated),
        normalize(golden),
        "generated devices.rs drifted from tests/golden/devices.rs"
    );
}

/// Strip trailing whitespace per line and collapse to a single trailing
/// newline so the comparison is not brittle against editor/tooling newline
/// handling.
fn normalize(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    for line in src.lines() {
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}
