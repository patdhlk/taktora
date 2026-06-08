//! Button -> motion command state machine for the EL7047 example.
//!
//! Pure logic: given the EL1008 button bits and the decoded EL7047 status,
//! decide the next [`El7047Control`]. Edge-triggered (a rising edge fires a
//! relative move); respects `Busy`, `Error`, and connector health.
//!
//! EL1008 channel map: ch1(bit0)=Index +, ch2(bit1)=Index -,
//! ch3(bit2)=Emergency-stop, ch4(bit3)=Fault-reset.

use crate::el7047::{El7047Control, El7047Status, start_type};

/// Per-cycle motion parameters from the CLI.
#[derive(Clone, Copy, Debug)]
pub struct MoveParams {
    /// Increments commanded per index press.
    pub step: i32,
    /// Move velocity (POS-interface raw units).
    pub velocity: i16,
    /// Acceleration ramp (POS-interface raw units).
    pub acceleration: u16,
    /// Deceleration ramp (POS-interface raw units).
    pub deceleration: u16,
}

/// Edge-triggered controller state.
#[derive(Clone, Copy, Debug, Default)]
pub struct Controller {
    prev_buttons: u8,
    // Latch for the Beckhoff positioning Execute input: we hold Execute high
    // from the rising edge that fires a move until the drive acknowledges with
    // Busy, then drop it so it is ready for the next rising edge. Forced low on
    // emergency-stop or loss of connector health (safe-state wins).
    execute_held: bool,
}

impl Controller {
    /// Compute the control surface for this cycle.
    ///
    /// `buttons` is the EL1008 input byte (bit0=ch1 ...). `healthy` is false
    /// when the connector reports Degraded/Down — we then clear Enable and
    /// refuse new moves (software safe-state; the SM watchdog is the
    /// hardware guarantee).
    pub fn step(
        &mut self,
        buttons: u8,
        status: &El7047Status,
        p: MoveParams,
        healthy: bool,
    ) -> El7047Control {
        let rising = buttons & !self.prev_buttons;
        self.prev_buttons = buttons;

        let estop = buttons & 0b0000_0100 != 0; // ch3 level-triggered
        let reset = rising & 0b0000_1000 != 0; // ch4 rising edge

        let mut ctrl = El7047Control {
            enable: healthy && !estop,
            reset,
            emergency_stop: estop,
            velocity: p.velocity,
            start_type: start_type::RELATIVE,
            acceleration: p.acceleration,
            deceleration: p.deceleration,
            ..Default::default()
        };

        // Drop a held Execute once the drive acknowledges the move with Busy;
        // it is then ready for the next rising edge.
        if self.execute_held && status.busy {
            self.execute_held = false;
        }

        let can_move = healthy && !estop && !status.busy && !status.error;
        if can_move {
            let dir = if rising & 0b0000_0001 != 0 {
                Some(1)
            } else if rising & 0b0000_0010 != 0 {
                Some(-1)
            } else {
                None
            };
            if let Some(sign) = dir {
                ctrl.target_position = p.step * sign;
                self.execute_held = true;
            }
        }

        // Safe-state wins: emergency-stop or loss of health forces the latch
        // (and the Execute output) low.
        if estop || !healthy {
            self.execute_held = false;
        }

        ctrl.execute = self.execute_held;
        ctrl
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> MoveParams {
        MoveParams {
            step: 3200,
            velocity: 1000,
            acceleration: 1000,
            deceleration: 1000,
        }
    }
    fn idle_status() -> El7047Status {
        El7047Status {
            ready: true,
            ready_to_execute: true,
            ..Default::default()
        }
    }

    #[test]
    fn rising_edge_ch1_fires_positive_relative_move() {
        let mut c = Controller::default();
        // first cycle: button low -> no move
        let ctrl = c.step(0b0000_0000, &idle_status(), params(), true);
        assert!(!ctrl.execute);
        // rising edge on ch1
        let ctrl = c.step(0b0000_0001, &idle_status(), params(), true);
        assert!(ctrl.execute);
        assert_eq!(ctrl.target_position, 3200);
        assert_eq!(ctrl.start_type, start_type::RELATIVE);
        assert!(ctrl.enable);
    }

    #[test]
    fn ch2_fires_negative_move() {
        let mut c = Controller::default();
        let _ = c.step(0, &idle_status(), params(), true);
        let ctrl = c.step(0b0000_0010, &idle_status(), params(), true);
        assert!(ctrl.execute);
        assert_eq!(ctrl.target_position, -3200);
    }

    #[test]
    fn no_new_move_while_busy() {
        let mut c = Controller::default();
        let _ = c.step(0, &idle_status(), params(), true);
        let busy = El7047Status {
            ready: true,
            busy: true,
            ..Default::default()
        };
        // rising edge but drive busy -> do not fire
        let ctrl = c.step(0b0000_0001, &busy, params(), true);
        assert!(!ctrl.execute);
    }

    #[test]
    fn ch3_emergency_stop_overrides() {
        let mut c = Controller::default();
        let ctrl = c.step(0b0000_0100, &idle_status(), params(), true);
        assert!(ctrl.emergency_stop);
        assert!(!ctrl.execute);
    }

    #[test]
    fn ch4_rising_edge_pulses_reset() {
        let mut c = Controller::default();
        let _ = c.step(0, &idle_status(), params(), true);
        let faulted = El7047Status {
            error: true,
            ..Default::default()
        };
        let ctrl = c.step(0b0000_1000, &faulted, params(), true);
        assert!(ctrl.reset);
    }

    #[test]
    fn execute_held_until_busy_then_second_press_fires() {
        let mut c = Controller::default();
        // settle prev_buttons low
        let _ = c.step(0, &idle_status(), params(), true);
        // press ch1 -> rising edge fires the move
        let ctrl = c.step(0b0000_0001, &idle_status(), params(), true);
        assert!(ctrl.execute, "rising edge should fire Execute");
        // next cycle: drive has not yet acknowledged (still !busy), button still
        // held (no new rising edge) -> Execute stays high (held).
        let ctrl = c.step(0b0000_0001, &idle_status(), params(), true);
        assert!(ctrl.execute, "Execute should stay high until Busy");
        // drive acknowledges with Busy -> Execute drops, ready for next edge.
        let busy = El7047Status {
            ready: true,
            busy: true,
            ..Default::default()
        };
        let ctrl = c.step(0b0000_0001, &busy, params(), true);
        assert!(!ctrl.execute, "Execute should drop once Busy observed");
        // move completes: not Busy, In-Target. Release button, then press again.
        let done = El7047Status {
            ready: true,
            in_target: true,
            ready_to_execute: true,
            ..Default::default()
        };
        let _ = c.step(0, &done, params(), true); // release
        let ctrl = c.step(0b0000_0001, &done, params(), true); // second press
        assert!(ctrl.execute, "a fresh press after completion should re-fire");
    }

    #[test]
    fn estop_clears_a_held_execute() {
        let mut c = Controller::default();
        let _ = c.step(0, &idle_status(), params(), true);
        // fire a move -> Execute held high
        let ctrl = c.step(0b0000_0001, &idle_status(), params(), true);
        assert!(ctrl.execute);
        // next cycle assert ch3 (estop): Execute must be forced low even though
        // it was held, and the latch must be cleared (safe-state wins).
        let ctrl = c.step(0b0000_0101, &idle_status(), params(), true);
        assert!(ctrl.emergency_stop);
        assert!(!ctrl.execute, "estop must force Execute low even when held");
    }

    #[test]
    fn health_degraded_clears_enable_and_blocks_moves() {
        let mut c = Controller::default();
        let _ = c.step(0, &idle_status(), params(), false);
        let ctrl = c.step(
            0b0000_0001,
            &idle_status(),
            params(),
            false, /* unhealthy */
        );
        assert!(!ctrl.enable);
        assert!(!ctrl.execute);
    }
}
