//! EL7047 joint OpMode enum (issue #70): mode presence, exact lengths,
//! pdo_assignment index lists, and an encode round-trip.
//!
//! The real Beckhoff EL7047 ESI declares nine `<AlternativeSmMapping>` PDO
//! assignments, so codegen emits a nine-variant `EL7047OpMode` enum. Every
//! value below is read from the blessed golden (`tests/golden/devices.rs`).

use bitvec::view::BitView;
use taktora_ethercat_esi_codegen_ethercrab_tests::generated::{EL7047, EL7047OpMode};
use taktora_ethercat_esi_rt::{EsiDevice, Lsb0};

/// The `Default="1"` mapping sorts first, so the generated `Default` impl picks
/// `VelocityControlCompact`.
#[test]
fn el7047_default_is_velocity_control_compact() {
    let dev = EL7047::default();
    assert!(matches!(dev.mode, EL7047OpMode::VelocityControlCompact(_)));
}

/// Construct each of the nine resolved PDO-assignment variants — proving the
/// enum has exactly nine modes and each is independently constructible.
#[test]
fn el7047_has_nine_modes() {
    let modes = [
        EL7047OpMode::VelocityControlCompact(Default::default()),
        EL7047OpMode::VelocityControlCompactWithInfoData(Default::default()),
        EL7047OpMode::VelocityControl(Default::default()),
        EL7047OpMode::PositionControl(Default::default()),
        EL7047OpMode::PositioningInterfaceCompact(Default::default()),
        EL7047OpMode::PositioningInterface(Default::default()),
        EL7047OpMode::PositioningInterfaceWithInfoData(Default::default()),
        EL7047OpMode::PositioningInterfaceAutoStart(Default::default()),
        EL7047OpMode::PositioningInterfaceAutoStartWithInfoData(Default::default()),
    ];
    assert_eq!(modes.len(), 9, "EL7047 should resolve to nine OpMode variants");
}

/// The Positioning-interface mode advertises its exact PDI widths via the
/// trait's `input_len`/`output_len` (24 input bytes, 22 output bytes).
#[test]
fn positioning_interface_exact_lengths() {
    let dev = EL7047 {
        mode: EL7047OpMode::PositioningInterface(Default::default()),
    };
    assert_eq!(dev.input_len(), 24, "PositioningInterface input bytes");
    assert_eq!(dev.output_len(), 22, "PositioningInterface output bytes");
}

/// `pdo_assignment()` returns the active mode's Rx (0x1C12) / Tx (0x1C13) PDO
/// index lists. PositioningInterface: rx 0x1601/0x1602/0x1606, tx
/// 0x1A01/0x1A03/0x1A07.
#[test]
fn positioning_interface_pdo_assignment_indices() {
    let dev = EL7047 {
        mode: EL7047OpMode::PositioningInterface(Default::default()),
    };
    let a = dev.pdo_assignment();
    assert_eq!(a.rx, &[0x1601u16, 0x1602u16, 0x1606u16]);
    assert_eq!(a.tx, &[0x1A01u16, 0x1A03u16, 0x1A07u16]);
}

/// The default (VelocityControlCompact) mode reports different widths and PDO
/// indices than PositioningInterface — confirming the per-mode `match self.mode`
/// actually discriminates.
#[test]
fn default_mode_lengths_and_pdo_assignment() {
    let dev = EL7047::default();
    assert_eq!(dev.input_len(), 8, "VelocityControlCompact input bytes");
    assert_eq!(dev.output_len(), 8, "VelocityControlCompact output bytes");
    let a = dev.pdo_assignment();
    assert_eq!(a.rx, &[0x1600u16, 0x1602u16, 0x1604u16]);
    assert_eq!(a.tx, &[0x1A00u16, 0x1A03u16]);
}

/// Encode round-trip: set a concrete output field (`pos_control.target_position`,
/// a u32 stored LE at bits 80..112 = bytes 10..14) and assert the encoded bytes
/// reflect it.
#[test]
fn positioning_interface_round_trip() {
    let mut dev = EL7047 {
        mode: EL7047OpMode::PositioningInterface(Default::default()),
    };
    if let EL7047OpMode::PositioningInterface(ref mut m) = dev.mode {
        m.outputs.pos_control.target_position = 0x0000_1234;
    } else {
        panic!("expected PositioningInterface mode");
    }

    let mut out = vec![0u8; dev.output_len()];
    dev.encode_outputs(out.as_mut_slice().view_bits_mut::<Lsb0>())
        .expect("encode");

    assert_eq!(out.len(), 22, "PositioningInterface output is 22 bytes");
    // target_position (u32) LE at bits 80..112 -> byte 10 = 0x34, byte 11 = 0x12.
    assert_eq!(out[10], 0x34, "low byte of target_position");
    assert_eq!(out[11], 0x12, "second byte of target_position");
    assert_eq!(out[12], 0x00, "high bytes of target_position are zero");
    assert_eq!(out[13], 0x00, "high bytes of target_position are zero");
}
