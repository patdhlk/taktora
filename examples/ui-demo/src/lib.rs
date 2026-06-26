//! Hardware-free domain model + simulation for the `ui-demo` example.
//!
//! This is the v1 validation slice of the MVVM UI connector (`FEAT_0092`),
//! decoupled from any fieldbus: a [`Simulator`] stands in for the real control
//! loop, ramping a simulated axis position and exposing the same MVVM surface a
//! real stepper driver would — a [`StepperViewModel`] property, an idempotent
//! `enable` command, and a non-idempotent `jog_relative` command gated by
//! [`Simulator::can_jog`].
//!
//! The producer binary (`src/main.rs`) wires this onto an
//! [`Executor`](taktora_executor::Executor) + a
//! [`UiConnector`](taktora_connector_ui::UiConnector); the egui View
//! (`examples/ui-demo-view`) binds purely over the published JSON contract.
//!
//! The simulation lives here (not in `main.rs`) precisely so it can be unit
//! tested without standing up iceoryx2 or an executor.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use taktora_connector_ui::{CommandParams, ImageEnum, ViewModel};

/// Lower simulated-position bound; the axis bounces between this and [`POS_MAX`].
pub const POS_MIN: f64 = 0.0;
/// Upper simulated-position bound.
pub const POS_MAX: f64 = 100.0;
/// Position units advanced per [`Simulator::step`] while `Running`.
pub const STEP_VELOCITY: f64 = 2.0;

/// Length (in ticks) of the recurring jog-availability cycle.
const BUSY_PERIOD: u64 = 100;
/// How many ticks at the start of each [`BUSY_PERIOD`] the axis is "busy" and
/// jogging is disabled — this is what makes the View's Jog button gray out and
/// re-enable on its own, demonstrating `CanExecute` gating end to end.
const BUSY_WINDOW: u64 = 20;

/// The simulated stepper lifecycle, lowered to a `u8` in the seqlock image.
///
/// C-like `#[repr(u8)]` enum: `ImageEnum` lowers it to its backing integer for
/// the torn-read-safe image, and the contract schema carries the variant table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ImageEnum)]
#[repr(u8)]
pub enum StepperState {
    /// Powered down; no motion, jogging disabled.
    Idle = 0,
    /// Enabled and ramping; jogging allowed except during the busy window.
    Running = 1,
    /// Reserved fault state (unused by the happy-path demo, present so the
    /// contract matches a realistic driver).
    Faulted = 2,
}

/// The ViewModel a UI subscribes to (`position`, `state`, `can_jog`).
///
/// One fixed-layout struct published as a latest-value (history-depth-1)
/// property on its own service. Matches the golden manifest fixture shipped with
/// `taktora-connector-ui-contract`.
#[derive(Clone, Debug, PartialEq, Serialize, ViewModel)]
pub struct StepperViewModel {
    /// Current simulated axis position in `[POS_MIN, POS_MAX]`.
    pub position: f64,
    /// Current lifecycle state.
    pub state: StepperState,
    /// Whether `jog_relative` will currently be accepted (drives the View's
    /// button enablement; mirrored onto the command's `CanExecute` gate).
    pub can_jog: bool,
}

/// Parameters for the idempotent `enable` command.
///
/// Idempotent: re-sending `enable` while already `Running` is a safe no-op, so
/// the client may auto-retry it on the same correlation id.
#[derive(Clone, Debug, PartialEq, Deserialize, CommandParams)]
#[command(idempotent)]
pub struct Enable {
    /// Force-enable even from a non-idle state (accepted, currently advisory).
    pub force: bool,
}

/// Parameters for the non-idempotent `jog_relative` command.
///
/// NOT idempotent: a resend would apply `delta` twice, so the client must not
/// auto-retry it across an epoch change (it surfaces `OutcomeUnknown` instead).
#[derive(Clone, Debug, PartialEq, Deserialize, CommandParams)]
pub struct JogRelative {
    /// Relative move applied to `position` (clamped to the axis bounds).
    pub delta: f64,
}

/// A pure, hardware-free model of the stepper control loop.
///
/// All mutation goes through [`step`](Self::step) (advance one control tick),
/// [`enable`](Self::enable), and [`jog`](Self::jog). Nothing here touches
/// iceoryx2, the executor, or wall-clock time, so it is fully unit testable.
#[derive(Clone, Debug)]
pub struct Simulator {
    position: f64,
    velocity: f64,
    state: StepperState,
    tick: u64,
}

impl Default for Simulator {
    fn default() -> Self {
        Self::new()
    }
}

impl Simulator {
    /// A fresh, powered-down simulator at the lower position bound.
    #[must_use]
    pub fn new() -> Self {
        Self {
            position: POS_MIN,
            velocity: STEP_VELOCITY,
            state: StepperState::Idle,
            tick: 0,
        }
    }

    /// Apply the idempotent `enable` effect: transition `Idle -> Running`.
    ///
    /// Re-enabling while already `Running` is a no-op (idempotent).
    pub fn enable(&mut self) {
        if self.state == StepperState::Idle {
            self.state = StepperState::Running;
        }
    }

    /// Apply a `jog_relative` effect: add `delta` to the position, clamped to
    /// the axis bounds.
    ///
    /// Gating is enforced upstream by the connector's `CanExecute` (mirrored
    /// from [`can_jog`](Self::can_jog)); a drained effect is always applied.
    pub fn jog(&mut self, delta: f64) {
        self.position = (self.position + delta).clamp(POS_MIN, POS_MAX);
    }

    /// Advance the simulation one control tick.
    ///
    /// While `Running`, the position ramps by [`STEP_VELOCITY`] and bounces off
    /// the bounds (a triangle wave), so a watching UI sees continuous motion.
    pub fn step(&mut self) {
        if self.state == StepperState::Running {
            self.position += self.velocity;
            if self.position >= POS_MAX {
                self.position = POS_MAX;
                self.velocity = -self.velocity.abs();
            } else if self.position <= POS_MIN {
                self.position = POS_MIN;
                self.velocity = self.velocity.abs();
            }
        }
        self.tick = self.tick.wrapping_add(1);
    }

    /// Whether jogging is currently allowed.
    ///
    /// True only while `Running` and outside the recurring busy window — so the
    /// flag flips on its own over time, exercising `CanExecute` gating in the
    /// View without any operator input.
    #[must_use]
    pub fn can_jog(&self) -> bool {
        self.state == StepperState::Running && (self.tick % BUSY_PERIOD) >= BUSY_WINDOW
    }

    /// Current position (test/inspection accessor).
    #[must_use]
    pub fn position(&self) -> f64 {
        self.position
    }

    /// Current lifecycle state (test/inspection accessor).
    #[must_use]
    pub fn state(&self) -> StepperState {
        self.state
    }

    /// Snapshot the current state as a [`StepperViewModel`] for publishing.
    #[must_use]
    pub fn view_model(&self) -> StepperViewModel {
        StepperViewModel {
            position: self.position,
            state: self.state,
            can_jog: self.can_jog(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_simulator_is_idle_at_origin_and_cannot_jog() {
        let sim = Simulator::new();
        assert_eq!(sim.state(), StepperState::Idle);
        assert_eq!(sim.position(), POS_MIN);
        assert!(!sim.can_jog(), "an idle axis must not allow jogging");
        let vm = sim.view_model();
        assert_eq!(vm.state, StepperState::Idle);
        assert!(!vm.can_jog);
    }

    #[test]
    fn idle_axis_does_not_move_on_step() {
        let mut sim = Simulator::new();
        sim.step();
        sim.step();
        assert_eq!(sim.position(), POS_MIN, "an idle axis must hold position");
    }

    #[test]
    fn enable_transitions_to_running_and_is_idempotent() {
        let mut sim = Simulator::new();
        sim.enable();
        assert_eq!(sim.state(), StepperState::Running);
        // Idempotent: a second enable does not change state or position.
        let before = sim.position();
        sim.enable();
        assert_eq!(sim.state(), StepperState::Running);
        assert_eq!(sim.position(), before);
    }

    #[test]
    fn running_axis_ramps_position_on_step() {
        let mut sim = Simulator::new();
        sim.enable();
        sim.step();
        assert_eq!(sim.position(), POS_MIN + STEP_VELOCITY);
    }

    #[test]
    fn position_stays_within_bounds_and_bounces() {
        let mut sim = Simulator::new();
        sim.enable();
        // Drive far more steps than the span; the triangle wave must stay bounded.
        let mut saw_high = false;
        let mut saw_low_after_high = false;
        for _ in 0..1000 {
            sim.step();
            assert!(
                (POS_MIN..=POS_MAX).contains(&sim.position()),
                "position {} escaped bounds",
                sim.position()
            );
            if sim.position() >= POS_MAX {
                saw_high = true;
            }
            if saw_high && sim.position() <= POS_MIN {
                saw_low_after_high = true;
            }
        }
        assert!(saw_high, "the ramp should reach the upper bound");
        assert!(
            saw_low_after_high,
            "the ramp should bounce back to the lower bound"
        );
    }

    #[test]
    fn jog_adds_delta_clamped_to_bounds() {
        let mut sim = Simulator::new();
        sim.enable();
        sim.jog(5.0);
        assert_eq!(sim.position(), POS_MIN + 5.0);
        // Clamp at the top.
        sim.jog(1000.0);
        assert_eq!(sim.position(), POS_MAX);
        // Clamp at the bottom.
        sim.jog(-1000.0);
        assert_eq!(sim.position(), POS_MIN);
    }

    #[test]
    fn can_jog_toggles_over_the_busy_window_while_running() {
        let mut sim = Simulator::new();
        sim.enable();
        // tick == 0 is inside the busy window -> cannot jog.
        assert!(!sim.can_jog(), "tick 0 is busy");
        // Advance out of the busy window.
        for _ in 0..BUSY_WINDOW {
            sim.step();
        }
        assert!(sim.can_jog(), "outside the busy window jogging is allowed");
        // Advance a full period back into the next busy window.
        for _ in 0..(BUSY_PERIOD - BUSY_WINDOW) {
            sim.step();
        }
        assert!(!sim.can_jog(), "the busy window recurs each period");
    }

    #[test]
    fn view_model_mirrors_can_jog() {
        let mut sim = Simulator::new();
        sim.enable();
        for _ in 0..BUSY_WINDOW {
            sim.step();
        }
        let vm = sim.view_model();
        assert_eq!(vm.can_jog, sim.can_jog());
        assert_eq!(vm.state, StepperState::Running);
    }
}
