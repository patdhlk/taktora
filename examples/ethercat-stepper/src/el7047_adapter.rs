//! Adapter between the domain control/status types in [`crate::el7047_domain`]
//! and the ESI-generated [`EL7047`] device in its `PositioningInterface` mode.
//!
//! The domain types ([`El7047Control`]/[`El7047Status`]) keep the pure
//! [`crate::control`] controller codegen-agnostic; this module is the only
//! place that touches the generated positioning-interface leaves.

use crate::el7047_domain::{El7047Control, El7047Status};
use crate::generated::{EL7047, EL7047OpMode};

/// Write the domain control surface into the device's `PositioningInterface`
/// output image.
///
/// The EL7047 **must** be in [`EL7047OpMode::PositioningInterface`] before
/// calling this function. Note that `EL7047OpMode::default()` is
/// `VelocityControlCompact`, **not** `PositioningInterface`; a
/// default-constructed device is therefore the wrong mode.
///
/// On a wrong-mode call: in debug builds this function panics immediately
/// (via `debug_assert!`); in release builds it is a silent no-op, which
/// means the output image retains whatever was written last cycle — for the
/// initial cycle that is a zeroed command, which could move a live drive
/// unexpectedly.
///
/// Fields not written by this function retain their previous value across
/// cycles (the `EL7047` device struct is reused). This is safe here because
/// the controller writes every mapped field on every cycle.
pub fn apply_control(dev: &mut EL7047, c: &El7047Control) {
    debug_assert!(
        matches!(dev.mode, EL7047OpMode::PositioningInterface(_)),
        "apply_control called on EL7047 not in PositioningInterface mode — output image would be silently wrong"
    );
    if let EL7047OpMode::PositioningInterface(ref mut m) = dev.mode {
        // ENC Control: only the manual "Set counter" datum is used.
        m.outputs.enc_control.control_set_counter = c.set_counter;
        m.outputs.enc_control.set_counter_value = c.set_counter_value as u32;
        // STM Control.
        m.outputs.stm_control.control_enable = c.enable;
        m.outputs.stm_control.control_reset = c.reset;
        // POS Control.
        m.outputs.pos_control.control_execute = c.execute;
        m.outputs.pos_control.control_emergency_stop = c.emergency_stop;
        m.outputs.pos_control.target_position = c.target_position as u32;
        m.outputs.pos_control.velocity = c.velocity;
        m.outputs.pos_control.start_type = c.start_type;
        m.outputs.pos_control.acceleration = c.acceleration;
        m.outputs.pos_control.deceleration = c.deceleration;
    }
}

/// Read the decoded `PositioningInterface` inputs into the domain status.
///
/// The EL7047 **must** be in [`EL7047OpMode::PositioningInterface`] before
/// calling this function. Note that `EL7047OpMode::default()` is
/// `VelocityControlCompact`, **not** `PositioningInterface`; a
/// default-constructed device is therefore the wrong mode.
///
/// On a wrong-mode call: in debug builds this function panics immediately
/// (via `debug_assert!`); in release builds it returns an all-default
/// [`El7047Status`] (all flags `false`, `actual_position` = 0), which
/// silently hides real drive state from the controller.
#[must_use]
pub fn read_status(dev: &EL7047) -> El7047Status {
    debug_assert!(
        matches!(dev.mode, EL7047OpMode::PositioningInterface(_)),
        "read_status called on EL7047 not in PositioningInterface mode — status would be all-default"
    );
    let mut s = El7047Status::default();
    if let EL7047OpMode::PositioningInterface(ref m) = dev.mode {
        // STM Status: drive readiness / fault flags.
        s.ready = m.inputs.stm_status.status_ready;
        s.error = m.inputs.stm_status.status_error;
        s.warning = m.inputs.stm_status.status_warning;
        s.motor_stall = m.inputs.stm_status.status_motor_stall;
        // POS Status: positioning progress + actual position.
        s.busy = m.inputs.pos_status.status_busy;
        s.in_target = m.inputs.pos_status.status_in_target;
        s.ready_to_execute = m.inputs.pos_status.status_ready_to_execute;
        s.actual_position = m.inputs.pos_status.actual_position as i32;
    }
    s
}
