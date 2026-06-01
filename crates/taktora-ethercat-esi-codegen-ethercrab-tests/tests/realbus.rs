//! T11 payoff: prove the three real trimmed Beckhoff demo devices
//! (EL1008 / EL2004 / EL3602) generate *compiling, correct* drivers
//! end-to-end, with registry dispatch.
//!
//! Each fixture in `esi/` is parsed at build time, codegen'd into
//! `OUT_DIR/devices.rs`, `include!`d via the crate's `generated` module, and
//! exercised here against known PDI bit-patterns:
//!
//! - EL1008: 8-channel digital input — `decode_inputs` from a 1-byte PDI.
//! - EL2004: 4-channel digital output — `encode_outputs` into a 1-byte PDI.
//! - EL3602: 2-channel analog input — `decode_inputs` from a 12-byte PDI,
//!   covering multi-PDO layout, BIT2 sub-fields and signed `i32` values.
//! - REGISTRY / `device_for`: identity-keyed dispatch over all four devices.

use bitvec::view::BitView;
use taktora_ethercat_esi_codegen_ethercrab_tests::generated::{
    EL1008, EL1008_REV00100000, EL2004, EL2004_REV00000000, EL3602, EL3602_REV00100000, REGISTRY,
    device_for,
};
use taktora_ethercat_esi_rt::{BitSlice, EsiDevice, EsiError, Identity, Lsb0};

// ---------------------------------------------------------------------------
// EL1008 — 8-channel digital INPUT (decode)
// ---------------------------------------------------------------------------

#[test]
fn el1008_decode_eight_channels() {
    let mut dev = EL1008::default();

    // 0b1010_1010 in Lsb0: bit i -> channel i+1.
    //   ch1=bit0=0, ch2=bit1=1, ch3=bit2=0, ch4=bit3=1,
    //   ch5=bit4=0, ch6=bit5=1, ch7=bit6=0, ch8=bit7=1.
    let bytes: [u8; 1] = [0b1010_1010];
    let bits: &BitSlice<u8, Lsb0> = bytes.view_bits::<Lsb0>();

    dev.decode_inputs(bits)
        .expect("EL1008 decode should succeed");

    assert!(!dev.channel_1.input, "ch1 (bit0) should be false");
    assert!(dev.channel_2.input, "ch2 (bit1) should be true");
    assert!(!dev.channel_3.input, "ch3 (bit2) should be false");
    assert!(dev.channel_4.input, "ch4 (bit3) should be true");
    assert!(!dev.channel_5.input, "ch5 (bit4) should be false");
    assert!(dev.channel_6.input, "ch6 (bit5) should be true");
    assert!(!dev.channel_7.input, "ch7 (bit6) should be false");
    assert!(dev.channel_8.input, "ch8 (bit7) should be true");

    assert_eq!(dev.input_len(), 1, "EL1008 has one input byte");
    assert_eq!(dev.output_len(), 0, "EL1008 has no outputs");
    assert_eq!(dev.identity(), EL1008_REV00100000);
}

// ---------------------------------------------------------------------------
// EL2004 — 4-channel digital OUTPUT (encode)
// ---------------------------------------------------------------------------

#[test]
fn el2004_encode_four_channels() {
    let mut dev = EL2004::default();
    // ch1=true, ch2=false, ch3=true, ch4=false.
    dev.channel_1.output = true;
    dev.channel_2.output = false;
    dev.channel_3.output = true;
    dev.channel_4.output = false;

    let mut buf = [0u8; 1];
    let bits: &mut BitSlice<u8, Lsb0> = buf.view_bits_mut::<Lsb0>();

    dev.encode_outputs(bits)
        .expect("EL2004 encode should succeed");

    // bit0=ch1=1, bit1=ch2=0, bit2=ch3=1, bit3=ch4=0 -> 0b0000_0101.
    assert_eq!(buf[0], 0b0000_0101, "encoded output byte mismatch");

    assert_eq!(dev.output_len(), 1, "EL2004 has one output byte");
    assert_eq!(dev.input_len(), 0, "EL2004 has no inputs");
    assert_eq!(dev.identity(), EL2004_REV00000000);
}

#[test]
fn el2004_encode_buffer_too_short() {
    let dev = EL2004::default();

    // Zero-length buffer: the device needs 4 bits.
    let mut buf: [u8; 0] = [];
    let bits: &mut BitSlice<u8, Lsb0> = buf.view_bits_mut::<Lsb0>();

    let err = dev
        .encode_outputs(bits)
        .expect_err("EL2004 encode should fail on a zero-length buffer");

    match err {
        EsiError::BufferTooShort {
            expected_bits,
            got_bits,
        } => {
            assert_eq!(expected_bits, 4, "EL2004 needs 4 output bits");
            assert_eq!(got_bits, 0, "supplied buffer had 0 bits");
        }
    }
}

// ---------------------------------------------------------------------------
// EL3602 — 2-channel analog INPUT (decode, multi-PDO + BIT2 + i32)
// ---------------------------------------------------------------------------

#[test]
fn el3602_decode_two_channels() {
    let mut dev = EL3602::default();

    // 12-byte (96-bit) PDI. Lsb0 byte layout (bit i -> byte i/8, pos i%8):
    //   ch1 underrange = bit0          = 1
    //   ch1 value (i32) bits 16..48    = 0x0001_0000 (= 65536)
    //   ch2 underrange = bit48         = 1
    //   ch2 value (i32) bits 64..96    = 0xFFFF_FFFE (= -2)
    // Computed bytes (see test comment for derivation):
    let bytes: [u8; 12] = [
        0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0xFE, 0xFF, 0xFF, 0xFF,
    ];
    let bits: &BitSlice<u8, Lsb0> = bytes.view_bits::<Lsb0>();

    dev.decode_inputs(bits)
        .expect("EL3602 decode should succeed");

    assert!(
        dev.ai_inputs_channel_1.underrange,
        "ch1 underrange should be true"
    );
    assert_eq!(
        dev.ai_inputs_channel_1.value, 0x0001_0000,
        "ch1 value should decode to 65536"
    );

    assert!(
        dev.ai_inputs_channel_2.underrange,
        "ch2 underrange should be true"
    );
    assert_eq!(
        dev.ai_inputs_channel_2.value, -2,
        "ch2 value should decode to -2 (signed i32)"
    );

    assert_eq!(dev.input_len(), 12, "EL3602 has 12 input bytes");
    assert_eq!(dev.output_len(), 0, "EL3602 has no outputs");
    assert_eq!(dev.identity(), EL3602_REV00100000);
}

#[test]
fn el3602_decode_buffer_too_short() {
    let mut dev = EL3602::default();

    // 11 bytes (88 bits); the device needs 96 bits.
    let bytes: [u8; 11] = [0u8; 11];
    let bits: &BitSlice<u8, Lsb0> = bytes.view_bits::<Lsb0>();

    let err = dev
        .decode_inputs(bits)
        .expect_err("EL3602 decode should fail on a short buffer");

    match err {
        EsiError::BufferTooShort {
            expected_bits,
            got_bits,
        } => {
            assert_eq!(expected_bits, 96);
            assert_eq!(got_bits, 88);
        }
    }
}

// ---------------------------------------------------------------------------
// Registry dispatch over all four generated devices.
// ---------------------------------------------------------------------------

#[test]
fn registry_has_all_devices() {
    // Four demo devices (EL1008/EL2004/EL3602/EL3001_like) plus the synthetic
    // ALT alternatives fixture = five esi/*.xml devices.
    assert_eq!(
        REGISTRY.len(),
        5,
        "all five esi/*.xml devices should register"
    );
}

#[test]
fn device_for_dispatches_known_identity() {
    // Use the generated identity const for EL2004.
    let dev = device_for(EL2004_REV00000000).expect("EL2004 identity should dispatch");
    assert_eq!(
        dev.identity(),
        EL2004_REV00000000,
        "dispatched device should report the requested identity"
    );
}

#[test]
fn device_for_constructed_identity_literal() {
    // An Identity literal equal to EL1008's identity must also dispatch,
    // proving lookup is by value and not by const aliasing.
    let id = Identity {
        vendor_id: 0x0000_0002,
        product_code: 66_072_658,
        revision: 0x0010_0000,
    };
    assert_eq!(
        id, EL1008_REV00100000,
        "literal should equal the EL1008 const"
    );

    let dev = device_for(id).expect("EL1008 identity literal should dispatch");
    assert_eq!(dev.identity(), EL1008_REV00100000);
}

#[test]
fn device_for_unknown_identity_is_none() {
    let bogus = Identity {
        vendor_id: 0xDEAD_BEEF,
        product_code: 0x0000_0000,
        revision: 0x0000_0000,
    };
    assert!(
        device_for(bogus).is_none(),
        "unknown identity must not dispatch"
    );
}

// ---------------------------------------------------------------------------
// Object-safety / &dyn smoke for one real device.
// ---------------------------------------------------------------------------

#[test]
fn el3602_object_safety_smoke() {
    let dev = EL3602::default();
    let dyn_ref: &dyn EsiDevice = &dev;
    assert_eq!(dyn_ref.input_len(), 12);
    assert_eq!(dyn_ref.identity(), EL3602_REV00100000);
}
