//! Transitional: assert the generated EL7047 `PositioningInterface` codec is
//! byte/field-identical to the hand-rolled `el7047.rs`, before the latter is
//! deleted. This is the correctness gate for `el7047_adapter`.

use bitvec::view::BitView;
use ethercat_stepper::el7047::{self, El7047Control, El7047Status};
use ethercat_stepper::el7047_adapter;
use ethercat_stepper::generated::{EL7047, EL7047OpMode};
use taktora_ethercat_esi_rt::{EsiDevice, Lsb0};

/// Build an EL7047 pinned to the `PositioningInterface` mode (the assignment
/// the example drives).
fn positioning_device() -> EL7047 {
    EL7047 {
        mode: EL7047OpMode::PositioningInterface(Default::default()),
    }
}

#[test]
fn generated_encode_matches_hand_rolled_relative_move() {
    let ctrl = El7047Control {
        enable: true,
        target_position: 12_345,
        velocity: 600,
        start_type: el7047::start_type::RELATIVE,
        acceleration: 1000,
        deceleration: 1000,
        execute: true,
        ..Default::default()
    };

    let hand = el7047::encode_control(&ctrl);

    let mut dev = positioning_device();
    el7047_adapter::apply_control(&mut dev, &ctrl);
    let mut g = vec![0u8; dev.output_len()];
    dev.encode_outputs(g.as_mut_slice().view_bits_mut::<Lsb0>())
        .expect("encode");

    assert_eq!(
        &g[..],
        &hand[..],
        "generated output image must match hand-rolled (relative move)"
    );
}

#[test]
fn generated_encode_matches_hand_rolled_set_counter() {
    // Exercise the ENC-control path (byte0 bit2 + value) plus emergency-stop.
    let ctrl = El7047Control {
        set_counter: true,
        set_counter_value: -1234,
        emergency_stop: true,
        reset: true,
        ..Default::default()
    };

    let hand = el7047::encode_control(&ctrl);

    let mut dev = positioning_device();
    el7047_adapter::apply_control(&mut dev, &ctrl);
    let mut g = vec![0u8; dev.output_len()];
    dev.encode_outputs(g.as_mut_slice().view_bits_mut::<Lsb0>())
        .expect("encode");

    assert_eq!(
        &g[..],
        &hand[..],
        "generated output image must match hand-rolled (set-counter)"
    );
}

#[test]
fn generated_decode_matches_hand_rolled_status() {
    // Known 24-byte input image with a spread of STM/POS flags + position.
    let mut img = [0u8; el7047::INPUT_LEN];
    // STM Status word at bytes 10-11: ready(bit1), warning(bit2), motor_stall(bit7).
    img[10] = 0b1000_0110;
    // POS Status word at bytes 12-13: busy(bit0), ready_to_execute(bit7).
    img[12] = 0b1000_0001;
    // Actual position bytes 14-17 = -4096.
    img[14..18].copy_from_slice(&(-4096i32).to_le_bytes());

    let hand = el7047::decode_status(&img).expect("hand decode");

    let mut dev = positioning_device();
    dev.decode_inputs(img.view_bits::<Lsb0>())
        .expect("generated decode");
    let got: El7047Status = el7047_adapter::read_status(&dev);

    assert_eq!(got, hand, "decoded status must match hand-rolled");
    // Spot-check the individual flags so a regression names the field.
    assert!(got.ready && got.warning && got.motor_stall);
    assert!(!got.error);
    assert!(got.busy && got.ready_to_execute);
    assert!(!got.in_target);
    assert_eq!(got.actual_position, -4096);
}

#[test]
fn generated_decode_matches_hand_rolled_error_state() {
    let mut img = [0u8; el7047::INPUT_LEN];
    // STM Status: ready(bit1) + error(bit3).
    img[10] = 0b0000_1010;
    // POS Status: in_target(bit1).
    img[12] = 0b0000_0010;
    img[14..18].copy_from_slice(&987_654i32.to_le_bytes());

    let hand = el7047::decode_status(&img).expect("hand decode");

    let mut dev = positioning_device();
    dev.decode_inputs(img.view_bits::<Lsb0>())
        .expect("generated decode");
    let got = el7047_adapter::read_status(&dev);

    assert_eq!(got, hand, "decoded status must match hand-rolled (error)");
    assert!(got.ready && got.error && got.in_target);
    assert!(!got.busy);
    assert_eq!(got.actual_position, 987_654);
}
