//! Button -> motion command state machine for the EL7047 example.
//!
//! Pure logic: given the EL1008 button bits and the decoded EL7047 status,
//! decide the next [`El7047Control`]. Edge-triggered (a rising edge fires a
//! relative move); respects `Busy`, `Error`, and connector health.
//!
//! EL1008 channel map: ch1(bit0)=Index +, ch2(bit1)=Index -,
//! ch3(bit2)=Emergency-stop, ch4(bit3)=Fault-reset, ch5(bit4)=Jog + (endless,
//! hold-to-run), ch6(bit5)=Jog - (endless). Jog overrides the index moves while
//! held and stops on release or stall (e.g. reaching a hard block).

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
    // Latch for the Beckhoff positioning Execute input. We hold Execute high for
    // the WHOLE move — from the rising edge that fires it until the drive has
    // gone Busy and then returned to not-Busy (move complete) — NOT just until
    // Busy. Dropping Execute mid-move aborts the travel on the EL7047, which
    // truncated each move to the few increments it covered in one control cycle.
    // Forced low on emergency-stop or loss of connector health (safe-state wins).
    execute_held: bool,
    // True once Busy has been observed for the in-flight move, so we can tell
    // "move not started yet" (hold) from "move finished" (release) — both have
    // busy == false.
    seen_busy: bool,
    // Target of the in-flight move. The POS-interface Target position must stay
    // valid for the WHOLE time Execute is held high — the EL7047 re-reads it and
    // would truncate the move if it reverted to 0 on the held cycles.
    held_target: i32,
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
        let jog_plus = buttons & 0b0001_0000 != 0; // ch5 level-triggered
        let jog_minus = buttons & 0b0010_0000 != 0; // ch6 level-triggered

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

        // ch5/ch6: hold-to-run endless jog (drive toward a hard block). This is
        // level-triggered and OVERRIDES the index moves while a jog button is
        // held. Releasing the button drops Execute (which stops the move), and a
        // stall — e.g. the carriage reaching the block — also stops it so we do
        // not grind against the stop.
        if jog_plus || jog_minus {
            // Cancel any in-flight index latch while jogging.
            self.execute_held = false;
            self.seen_busy = false;
            let can_jog =
                healthy && !estop && !status.error && !status.motor_stall && (jog_plus ^ jog_minus); // both pressed -> ambiguous -> stop
            if can_jog {
                ctrl.start_type = if jog_plus {
                    start_type::ENDLESS_PLUS
                } else {
                    start_type::ENDLESS_MINUS
                };
                ctrl.execute = true;
            }
            // target_position is ignored for endless moves; leave it 0.
            return ctrl;
        }

        // Track move progress and release Execute only when the move completes:
        // Busy has been observed AND has since cleared. Holding Execute through
        // the whole Busy window is required — dropping it early aborts the move.
        if self.execute_held {
            if status.busy {
                self.seen_busy = true;
            }
            if self.seen_busy && !status.busy {
                self.execute_held = false;
            }
        }

        // Fire a new move only when idle (drive ready, not busy, not already
        // executing a held move).
        let can_move = healthy && !estop && !status.busy && !status.error && !self.execute_held;
        if can_move {
            let dir = if rising & 0b0000_0001 != 0 {
                Some(1)
            } else if rising & 0b0000_0010 != 0 {
                Some(-1)
            } else {
                None
            };
            if let Some(sign) = dir {
                self.held_target = p.step * sign;
                self.execute_held = true;
                self.seen_busy = false;
            }
        }

        // Safe-state wins: emergency-stop or loss of health forces the latch
        // (and the Execute output) low.
        if estop || !healthy {
            self.execute_held = false;
        }

        ctrl.execute = self.execute_held;
        // Hold the target steady for as long as Execute is asserted; the drive
        // latched the move on the rising edge but keeps reading this field.
        ctrl.target_position = if self.execute_held {
            self.held_target
        } else {
            0
        };
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
    fn execute_held_through_whole_move_then_second_press_fires() {
        let mut c = Controller::default();
        // settle prev_buttons low
        let _ = c.step(0, &idle_status(), params(), true);
        // press ch1 -> rising edge fires the move
        let ctrl = c.step(0b0000_0001, &idle_status(), params(), true);
        assert!(ctrl.execute, "rising edge should fire Execute");
        assert_eq!(ctrl.target_position, 3200);
        // before the drive reports Busy, Execute stays high and target stable.
        let ctrl = c.step(0b0000_0001, &idle_status(), params(), true);
        assert!(ctrl.execute, "Execute held before Busy");
        assert_eq!(ctrl.target_position, 3200, "target stable while held");
        // drive is now Busy (moving). Execute MUST stay high for the whole move —
        // dropping it here aborts the travel on the EL7047.
        let busy = El7047Status {
            ready: true,
            busy: true,
            ..Default::default()
        };
        let ctrl = c.step(0b0000_0001, &busy, params(), true);
        assert!(
            ctrl.execute,
            "Execute must stay high WHILE Busy (whole move)"
        );
        assert_eq!(ctrl.target_position, 3200, "target stable while moving");
        // still moving a few cycles later -> still held.
        let ctrl = c.step(0b0000_0001, &busy, params(), true);
        assert!(ctrl.execute, "Execute still high mid-move");
        // move completes: Busy clears, In-Target set -> Execute released.
        let done = El7047Status {
            ready: true,
            in_target: true,
            ready_to_execute: true,
            ..Default::default()
        };
        let ctrl = c.step(0b0000_0001, &done, params(), true);
        assert!(!ctrl.execute, "Execute released once the move completes");
        // release button, then press again -> a fresh move fires.
        let _ = c.step(0, &done, params(), true); // release
        let ctrl = c.step(0b0000_0001, &done, params(), true); // second press
        assert!(
            ctrl.execute,
            "a fresh press after completion should re-fire"
        );
        assert_eq!(ctrl.target_position, 3200);
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
    fn ch5_jogs_endless_plus_while_held() {
        let mut c = Controller::default();
        let ctrl = c.step(0b0001_0000, &idle_status(), params(), true);
        assert!(ctrl.execute);
        assert_eq!(ctrl.start_type, start_type::ENDLESS_PLUS);
        assert!(ctrl.enable);
        // release -> Execute drops (move stops).
        let ctrl = c.step(0, &idle_status(), params(), true);
        assert!(!ctrl.execute);
    }

    #[test]
    fn ch6_jogs_endless_minus() {
        let mut c = Controller::default();
        let ctrl = c.step(0b0010_0000, &idle_status(), params(), true);
        assert!(ctrl.execute);
        assert_eq!(ctrl.start_type, start_type::ENDLESS_MINUS);
    }

    #[test]
    fn jog_stops_on_stall_at_block() {
        let mut c = Controller::default();
        // jogging
        let ctrl = c.step(0b0001_0000, &idle_status(), params(), true);
        assert!(ctrl.execute);
        // carriage reaches the block -> motor stalls -> stop pushing.
        let stalled = El7047Status {
            ready: true,
            motor_stall: true,
            ..Default::default()
        };
        let ctrl = c.step(0b0001_0000, &stalled, params(), true);
        assert!(!ctrl.execute, "must stop driving once stalled on the block");
    }

    #[test]
    fn both_jog_buttons_stop() {
        let mut c = Controller::default();
        let ctrl = c.step(0b0011_0000, &idle_status(), params(), true);
        assert!(!ctrl.execute, "ambiguous direction -> stop");
    }

    #[test]
    fn jog_overrides_and_cancels_an_index_move() {
        let mut c = Controller::default();
        // start an index move (Execute held)
        let _ = c.step(0, &idle_status(), params(), true);
        let ctrl = c.step(0b0000_0001, &idle_status(), params(), true);
        assert!(ctrl.execute);
        assert_eq!(ctrl.start_type, start_type::RELATIVE);
        // now hold ch5: jog takes over with an endless move.
        let ctrl = c.step(0b0001_0000, &idle_status(), params(), true);
        assert!(ctrl.execute);
        assert_eq!(ctrl.start_type, start_type::ENDLESS_PLUS);
    }

    #[test]
    fn jog_blocked_by_estop_and_health() {
        let mut c = Controller::default();
        // estop while jogging -> no drive, no enable.
        let ctrl = c.step(0b0001_0100, &idle_status(), params(), true);
        assert!(!ctrl.execute);
        assert!(!ctrl.enable);
        // unhealthy while jogging -> no drive.
        let ctrl = c.step(0b0001_0000, &idle_status(), params(), false);
        assert!(!ctrl.execute);
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
