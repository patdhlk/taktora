//! Per-axis NC runtime (`REQ_0857`, `REQ_0858`).
//!
//! [`AxisRuntime`] is the per-axis glue ticked every fieldbus cycle. It runs the
//! `CiA` 402 power state machine toward an operator target, performs *bumpless*
//! start (`REQ_0858`) by seeding the trajectory generator to the live actual
//! while the drive is not yet enabled, drives the [`motion-core`] generator once
//! enabled, scales commanded units to drive increments (`CSP`, mode 8), and
//! publishes a [`proto::AxisStatus`].
//!
//! Coupling across axes (`master` feed) and full command/token mapping arrive in
//! later tasks; this type carries minimal token/status fields now so those
//! extensions slot in without reshaping the struct.
//!
//! [`motion-core`]: taktora_motion_core

use taktora_cia402::{Cia402Drive, PowerStateMachine, PowerTarget};
use taktora_motion_core::profile::{SCurveState, TrapState, VelocityMove};
use taktora_motion_core::{Axis, AxisState as CoreAxisState, Limits, Motion};
use taktora_motion_proto as proto;

use crate::scale::{AxisScale, Unwrapper};

/// Per-axis runtime: power state machine + bumpless trajectory + scaling.
///
/// Holds, for one axis, the `CiA` 402 [`PowerStateMachine`], the [`AxisScale`]
/// and [`Unwrapper`] that bridge engineering units and drive increments, the
/// [`motion-core`] trajectory state (held as a whole [`Axis`] so Task 11 can
/// couple it as a master/slave), token bookkeeping (minimal for now), and the
/// published [`proto::AxisStatus`].
///
/// [`motion-core`]: taktora_motion_core
#[derive(Debug, Clone)]
pub struct AxisRuntime {
    axis_id: proto::AxisId,
    power: PowerStateMachine,
    scale: AxisScale,
    unwrapper: Unwrapper,
    /// The trajectory generator state. Held as a whole [`Axis`] (not a bare
    /// [`Motion`]) so Task 11 coupling can read its `state` as a master and set
    /// `master_idx`.
    arm: Axis,
    /// Whether the drive reported `OperationEnabled` on the most recent tick's
    /// statusword read (i.e. the state *before* this cycle's exchange).
    enabled: bool,
    /// Latched safe-state reaction (`REQ_0861`): once a fault drives this axis
    /// (directly or as an engaged-downstream slave) to Quick Stop, its
    /// published state is forced to `ErrorStop` until power is re-requested.
    error_stop: bool,
    /// Active correlation token (`REQ_0855`). A command whose `token` differs
    /// from this is a rising edge; the same token is idempotent.
    last_token: proto::Token,
    token_state: proto::TokenState,
    /// `true` while a finite (point-to-point) move is the active motion, so
    /// `tick` can flip the token `Active` -> `Done` once the profile completes.
    move_finite: bool,
    /// The token superseded on the most recent rising edge while it was still
    /// `Active` (Aborting buffer mode). Observation hook for the commander /
    /// tests: the single-slot [`proto::AxisStatus`] only carries the *current*
    /// token, so the aborted predecessor is surfaced here.
    last_aborted_token: Option<proto::Token>,
    /// Published per-cycle status (`REQ_0855`).
    status: proto::AxisStatus,
}

/// `CiA` 402 `CSP` modes-of-operation value (`0x6060` = 8).
const MODE_CSP: u8 = 8;

impl AxisRuntime {
    /// New runtime for `axis_id` with the given drive-boundary `scale`.
    ///
    /// Starts disabled (power target [`PowerTarget::Disabled`]) with an
    /// `Idle(0)` trajectory; the first ticks reseed the trajectory to the live
    /// actual until the drive enables (bumpless, `REQ_0858`).
    #[must_use]
    pub fn new(axis_id: proto::AxisId, scale: AxisScale) -> Self {
        Self {
            axis_id,
            power: PowerStateMachine::new(PowerTarget::Disabled),
            scale,
            unwrapper: Unwrapper::default(),
            arm: Axis::new(Motion::Idle(0.0)),
            enabled: false,
            error_stop: false,
            last_token: 0,
            token_state: proto::TokenState::Idle,
            move_finite: false,
            last_aborted_token: None,
            status: proto::AxisStatus {
                axis_id,
                state: proto::AxisState::Disabled,
                token_state: proto::TokenState::Idle,
                last_token: 0,
                actual_pos: 0.0,
                actual_vel: 0.0,
                cmd_pos: 0.0,
                error_code: 0,
            },
        }
    }

    /// Advance one fieldbus cycle: run the power machine, unwrap feedback, do
    /// bumpless seeding while disabled, tick the trajectory once enabled, and
    /// write the scaled `CSP` target.
    ///
    /// `master` is the coupled master's commanded [`CoreAxisState`] for this
    /// same cycle (`None` if this axis is uncoupled). Slaves read it to follow
    /// their master same-cycle (`REQ_0862`); the trajectory generator consumes
    /// it via [`Motion::update`].
    ///
    /// While the drive is not yet enabled this reseeds `arm.motion` to
    /// `Idle(actual)` every cycle for bumpless start (`REQ_0858`), which
    /// supersedes any motion set by an [`apply_command`](Self::apply_command)
    /// issued before enable — issue `Power` and await `OperationEnabled` first.
    pub fn tick<D>(&mut self, image: &mut [u8], drive: &D, dt: f64, master: Option<CoreAxisState>)
    where
        D: Cia402Drive<Image = [u8]>,
    {
        // 1. Read statusword; 2. compute + write controlword and CSP mode.
        let sw = drive.statusword(image);
        drive.set_controlword(image, self.power.next_controlword(sw));
        drive.set_mode(image, MODE_CSP);

        // 3. Unwrap the actual into continuous units.
        let raw = drive.actual_position(image);
        let accum = self.unwrapper.update(raw);
        let actual_units = self.scale.to_units(accum);

        // 4. Latch the enabled flag from the statusword read this cycle.
        self.enabled = PowerStateMachine::is_enabled(sw);

        // 5/6. Decide the commanded position.
        let commanded = if self.enabled {
            // Tick the (already-seeded-at-actual) trajectory, feeding the
            // coupled master's commanded state (`None` if uncoupled), then fold
            // in any superimposed corrective overlay (`MC_MoveSuperimposed`).
            // This runtime ticks `arm.motion` directly rather than through an
            // `AxisGroup`, so the overlay must be applied here — exactly as
            // `AxisGroup::tick` does — or a `Superimpose` command is a silent
            // no-op. `SCurveState::update` is allocation-free, so this stays
            // no-alloc.
            let base = self.arm.motion.update(dt, master);
            let next = self.arm.apply_superimposed(base, dt);
            self.arm.state = next;
            // Finite-move completion: a `MoveAbsolute`/`MoveRelative` profile
            // reports `Standstill` from its generator once `done()`; flip the
            // still-`Active` token to `Done` (`REQ_0855`).
            if self.move_finite
                && self.token_state == proto::TokenState::Active
                && matches!(
                    self.arm.status(),
                    taktora_motion_core::AxisStatus::Standstill
                )
            {
                self.token_state = proto::TokenState::Done;
                self.move_finite = false;
            }
            next.pos
        } else {
            // Bumpless start (`REQ_0858`): every disabled cycle reseed the
            // generator to hold at the live actual, so the first commanded
            // setpoint after enable equals the live actual (no lurch to 0).
            self.arm.motion = Motion::Idle(actual_units);
            self.arm.state = CoreAxisState::at(actual_units);
            actual_units
        };

        // 6 (cont). Write the scaled CSP target.
        drive.set_target_position(image, self.scale.to_increments(commanded));

        // 7. Publish status (minimal; full token/command mapping is Task 12).
        // A latched safe-state reaction overrides the nominal state until power
        // is re-requested (`REQ_0861`).
        self.status.state = if self.error_stop {
            proto::AxisState::ErrorStop
        } else if self.enabled {
            map_status(self.arm.status())
        } else {
            proto::AxisState::Disabled
        };
        self.status.token_state = self.token_state;
        self.status.last_token = self.last_token;
        self.status.actual_pos = actual_units;
        // `CSP` drives expose only position feedback; until a real connector
        // surfaces velocity, publish the commanded velocity as a proxy (with
        // the perfect-follower model the two coincide). REQ for true feedback
        // velocity differentiation is deferred to the EtherCAT connector phase.
        self.status.actual_vel = self.arm.state.vel;
        self.status.cmd_pos = commanded;
    }

    /// Set the power target to `Enabled` (`on`) or `Disabled` (operator
    /// `MC_Power`). Clears any latched safe-state reaction (re-arm).
    pub const fn request_power(&mut self, on: bool) {
        self.error_stop = false;
        self.power.set_target(if on {
            PowerTarget::Enabled
        } else {
            PowerTarget::Disabled
        });
    }

    /// Whether the drive reported `OperationEnabled` on the last tick.
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Command the safe-state reaction: walk the power machine to Quick Stop
    /// (`REQ_0861`). Driven by the engaged-downstream fault path in
    /// [`crate::cycle::NcCycle::step`].
    pub(crate) const fn request_quick_stop(&mut self) {
        self.power.set_target(PowerTarget::QuickStop);
    }

    /// Latch the published state to `ErrorStop` (`REQ_0861`) and reflect it in
    /// the already-published status immediately, so a fault detected after this
    /// cycle's `tick` still surfaces as `ErrorStop` in the same `step`.
    pub(crate) const fn force_error_stop(&mut self) {
        self.error_stop = true;
        self.status.state = proto::AxisState::ErrorStop;
    }

    /// The current `CiA` 402 power target (inspection helper for the fault
    /// path and tests).
    #[must_use]
    pub(crate) const fn power_target(&self) -> PowerTarget {
        self.power.target()
    }

    /// Replace the active trajectory generator (`REQ_0855` command mapping).
    pub(crate) const fn set_motion(&mut self, motion: Motion) {
        self.arm.motion = motion;
    }

    /// The token superseded on the most recent rising edge while still
    /// `Active` (Aborting buffer mode observation hook, `REQ_0855`).
    #[must_use]
    pub(crate) const fn last_aborted_token(&self) -> Option<proto::Token> {
        self.last_aborted_token
    }

    /// Apply one [`proto::AxisCommand`] with token correlation and the Aborting
    /// buffer mode (`REQ_0855`).
    ///
    /// `direct_master` is this axis's resolved direct coupling master (from the
    /// topology), consumed by `GearIn`; `None` if the axis has no declared
    /// master.
    ///
    /// # Token semantics
    ///
    /// A command whose `token` equals the current `last_token` is **not** a
    /// rising edge and is ignored (idempotent re-issue). A differing token is a
    /// rising edge: it is latched as `last_token`, and if it supersedes a still
    /// -`Active` token that predecessor is recorded as `Aborted` (only one move
    /// in flight — Aborting buffer mode). The new token's state is then set per
    /// the mapping below.
    ///
    /// # Mapping (v1)
    ///
    /// - `Power` -> `request_power(true)`; token `Active`.
    /// - `Reset` -> `request_power(true)` (clears the error-stop latch / re-arms
    ///   to `Enabled`); token `Active`.
    /// - `MoveVelocity` -> [`Motion::Velocity`]; `ContinuousMotion`, `Active`.
    /// - `MoveAbsolute` / `MoveRelative` -> a point-to-point profile (`SCurve`
    ///   if `jerk > 0`, else `Trapezoid`) from `commanded_state`;
    ///   `DiscreteMotion`, `Active` until `done()` -> `Done`. Plan `Err` (e.g.
    ///   non-positive limits, out-of-window target) -> `Error`, no fault.
    /// - `Stop` / `Halt` -> controlled ramp to zero velocity; token stays
    ///   `Active` until a new token (`Halt` is re-commandable in `PLCopen`; v1
    ///   treats both identically — the nuance is deferred).
    /// - `GearIn` -> [`Motion::Gear`] (ratio = `params.target_pos`) coupled to
    ///   `direct_master`; `SynchronizedMotion`, `Active`. No master -> `Error`.
    /// - `CamIn`, `FlyingSaw` -> **infeasible at this layer in v1** -> `Error`,
    ///   no fault, active motion unchanged (a runtime `AxisCommand` cannot carry
    ///   a `&'static` cam table nor the full flying-saw engagement context).
    /// - `Superimpose` -> [`Axis::superimpose`]; `Active`, `Error` on plan
    ///   failure.
    ///
    /// An infeasible/invalid command sets the token to `Error`, does **not**
    /// fault the axis, and does **not** change the active motion.
    ///
    /// # Faulted axis (`ErrorStop`)
    ///
    /// While the safe-state latch is set (the axis is published as `ErrorStop`,
    /// `REQ_0861`), only `Power` and `Reset` are accepted (they clear the latch /
    /// re-arm). Any other command is **rejected** with token `Error` — the active
    /// motion and the latch are left untouched. The rejection token is still
    /// recorded as the rising edge so the commander observes it. This prevents a
    /// move reporting `Active` while the published state is forced to `ErrorStop`.
    ///
    /// # Not-yet-enabled axis (disabled-move clobber)
    ///
    /// A motion command applied while the drive is **not yet enabled** is
    /// superseded by bumpless hold before it can tick: [`AxisRuntime::tick`]
    /// reseeds `arm.motion` to `Idle(actual)` every disabled cycle (`REQ_0858`),
    /// overwriting the freshly-set motion (even though `apply_command` set the
    /// token `Active`). For v1 this is acceptable — the commander is expected to
    /// issue `Power` and await `OperationEnabled` *before* any motion command.
    pub(crate) fn apply_command(&mut self, cmd: &proto::AxisCommand, direct_master: Option<usize>) {
        // Same token => not a rising edge; idempotent.
        if cmd.token == self.last_token && self.token_state != proto::TokenState::Idle {
            return;
        }

        // Rising edge: a still-`Active` predecessor is superseded (Aborting).
        if self.token_state == proto::TokenState::Active {
            self.last_aborted_token = Some(self.last_token);
        }
        self.last_token = cmd.token;

        // Fault guard (`REQ_0861`): while the safe-state latch (`ErrorStop`) is
        // set, the axis has no motion authority — only `Power`/`Reset` (which
        // clear the latch / re-arm) are accepted. Any other command is rejected
        // with token `Error` (recording this token as the rising edge so the
        // commander sees the rejection), leaving `arm.motion` and the latch
        // untouched. Without this guard a move would set `token_state = Active`
        // while the published `state` is forced to `ErrorStop` — a contradiction
        // (reported `Active` but cannot run).
        if self.error_stop
            && !matches!(
                cmd.kind,
                proto::CommandKind::Power | proto::CommandKind::Reset
            )
        {
            self.token_state = proto::TokenState::Error;
            return;
        }

        let p = &cmd.params;
        match cmd.kind {
            proto::CommandKind::Power | proto::CommandKind::Reset => {
                self.request_power(true);
                self.move_finite = false;
                self.token_state = proto::TokenState::Active;
            }
            proto::CommandKind::MoveVelocity => {
                self.set_motion(Motion::Velocity(VelocityMove::new(
                    self.commanded_state(),
                    p.velocity,
                    p.accel,
                )));
                self.move_finite = false;
                self.token_state = proto::TokenState::Active;
            }
            proto::CommandKind::Stop | proto::CommandKind::Halt => {
                // Controlled stop: ramp velocity to zero, stay busy (`Active`).
                self.set_motion(Motion::Velocity(VelocityMove::new(
                    self.commanded_state(),
                    0.0,
                    p.accel,
                )));
                self.move_finite = false;
                self.token_state = proto::TokenState::Active;
            }
            proto::CommandKind::MoveAbsolute => {
                self.plan_point_to_point(p, p.target_pos);
            }
            proto::CommandKind::MoveRelative => {
                let target = self.commanded_state().pos + p.target_pos;
                self.plan_point_to_point(p, target);
            }
            proto::CommandKind::GearIn => {
                if direct_master.is_some() {
                    self.set_motion(Motion::Gear(taktora_motion_core::couple::Gear::new(
                        p.target_pos,
                    )));
                    self.move_finite = false;
                    self.token_state = proto::TokenState::Active;
                } else {
                    // No declared master to follow: infeasible, no fault.
                    self.token_state = proto::TokenState::Error;
                }
            }
            proto::CommandKind::Superimpose => match Limits::new(
                p.velocity,
                p.accel,
                p.jerk,
                f64::NEG_INFINITY,
                f64::INFINITY,
            ) {
                Ok(limits) if self.arm.superimpose(p.target_pos, limits).is_ok() => {
                    self.token_state = proto::TokenState::Active;
                }
                _ => self.token_state = proto::TokenState::Error,
            },
            // Infeasible at this layer in v1: no fault, motion unchanged.
            proto::CommandKind::CamIn | proto::CommandKind::FlyingSaw => {
                self.token_state = proto::TokenState::Error;
            }
        }
    }

    /// Plan a point-to-point move to `target` from the commanded state, choosing
    /// `SCurve` when a jerk limit is supplied and `Trapezoid` otherwise. On a
    /// plan error (`NonPositiveLimit`, `TargetOutOfLimits`, ...) the token is set
    /// to `Error` without faulting the axis or changing the active motion.
    fn plan_point_to_point(&mut self, p: &proto::CommandParams, target: f64) {
        // Build kinematic limits from params; reject non-positive limits.
        let Ok(limits) = Limits::new(
            p.velocity,
            p.accel,
            p.jerk,
            f64::NEG_INFINITY,
            f64::INFINITY,
        ) else {
            self.token_state = proto::TokenState::Error;
            return;
        };
        let start = self.commanded_state();
        let planned = if p.jerk > 0.0 {
            SCurveState::plan(start, target, limits).map(Motion::SCurve)
        } else {
            TrapState::plan(start, target, limits).map(Motion::Trapezoid)
        };
        match planned {
            Ok(motion) => {
                self.set_motion(motion);
                self.move_finite = true;
                self.token_state = proto::TokenState::Active;
            }
            Err(_) => self.token_state = proto::TokenState::Error,
        }
    }

    /// The commanded set-state this runtime last produced (read by inter-axis
    /// coupling when this axis is a master).
    #[must_use]
    pub(crate) const fn commanded_state(&self) -> CoreAxisState {
        self.arm.state
    }

    /// The most recently published status (`REQ_0855`).
    #[must_use]
    pub const fn status(&self) -> proto::AxisStatus {
        self.status
    }

    /// The current token state, before any tick publishes it (test inspection).
    #[cfg(test)]
    pub(crate) const fn token_state_for_test(&self) -> proto::TokenState {
        self.token_state
    }

    /// Whether the safe-state latch is set (test inspection).
    #[cfg(test)]
    pub(crate) const fn error_stop_for_test(&self) -> bool {
        self.error_stop
    }

    /// This runtime's axis id.
    #[must_use]
    pub const fn axis_id(&self) -> proto::AxisId {
        self.axis_id
    }
}

/// Map the [`motion-core`] read-model status onto the published `proto` state.
///
/// [`motion-core`]: taktora_motion_core
const fn map_status(status: taktora_motion_core::AxisStatus) -> proto::AxisState {
    use taktora_motion_core::AxisStatus as Core;
    match status {
        Core::Standstill => proto::AxisState::Standstill,
        Core::DiscreteMotion => proto::AxisState::DiscreteMotion,
        Core::ContinuousMotion => proto::AxisState::ContinuousMotion,
        Core::SynchronizedMotion => proto::AxisState::SynchronizedMotion,
        Core::ErrorStop => proto::AxisState::ErrorStop,
        // `Core::Disabled` and any future (`#[non_exhaustive]`) variant we don't
        // yet map fall through to `Disabled` — the safe no-motion-authority
        // default.
        _ => proto::AxisState::Disabled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::{MockCyclicFieldbus, MockDrive};
    use taktora_cia402::Cia402Drive;
    use taktora_cyclic_fieldbus::CyclicFieldbus;

    #[test]
    fn first_setpoint_after_enable_equals_actual() {
        // Pre-position the virtual drive's actual at 7777 increments while disabled.
        let mut bus = MockCyclicFieldbus::new(1);
        bus.with_image_mut(|img| MockDrive::for_axis(0).set_target_position(img, 0));
        // Manually park actual at 7777 (encoder offset on a disabled axis).
        bus.with_image_mut(|img| img[9..13].copy_from_slice(&7777i32.to_le_bytes()));

        let scale = crate::scale::AxisScale {
            inc_per_unit: 1000.0,
            zero_offset: 0,
        };
        let mut axis = AxisRuntime::new(0, scale);
        axis.request_power(true);

        // Run until enabled; capture the first commanded increment after enable.
        let mut first_cmd_after_enable = None;
        let drive = MockDrive::for_axis(0);
        for _ in 0..12 {
            let dt = 0.002;
            bus.with_image_mut(|img| axis.tick(img, &drive, dt, None));
            pollster::block_on(bus.exchange()).unwrap();
            if axis.is_enabled() && first_cmd_after_enable.is_none() {
                let t = bus.with_image(|img| MockDrive::for_axis(0).actual_position(img));
                first_cmd_after_enable = Some(t);
            }
        }
        // Bumpless: commanded target seeded from actual (7777), no lurch to 0.
        assert_eq!(first_cmd_after_enable, Some(7777));
    }

    fn cmd_params(target_pos: f64, velocity: f64, accel: f64, jerk: f64) -> proto::CommandParams {
        proto::CommandParams {
            target_pos,
            velocity,
            accel,
            jerk,
        }
    }

    /// Drive `axis` (id 0, unit scale, actual parked at 0) up to enabled.
    fn enabled_axis() -> (AxisRuntime, MockCyclicFieldbus) {
        let mut bus = MockCyclicFieldbus::new(1);
        let scale = crate::scale::AxisScale {
            inc_per_unit: 1000.0,
            zero_offset: 0,
        };
        let mut axis = AxisRuntime::new(0, scale);
        axis.request_power(true);
        let drive = MockDrive::for_axis(0);
        for _ in 0..16 {
            bus.with_image_mut(|img| axis.tick(img, &drive, 0.002, None));
            pollster::block_on(bus.exchange()).unwrap();
            if axis.is_enabled() {
                break;
            }
        }
        assert!(axis.is_enabled(), "axis should enable");
        (axis, bus)
    }

    #[test]
    fn superimpose_shifts_commanded_position() {
        // Fix 1: a Superimpose command must actually fold its additive overlay
        // into the commanded position; before the fix it was a silent no-op.
        let (mut axis, mut bus) = enabled_axis();
        let drive = MockDrive::for_axis(0);
        let dt = 0.002;

        // Baseline commanded position (overlay not yet issued, axis at rest at 0).
        let base = axis.status().cmd_pos;

        // MC_MoveSuperimposed: delta = +10 units, jerk-limited.
        let cmd = proto::AxisCommand {
            axis_id: 0,
            token: 1,
            kind: proto::CommandKind::Superimpose,
            params: cmd_params(10.0, 50.0, 500.0, 5000.0),
        };
        axis.apply_command(&cmd, None);

        // Step several cycles; the overlay must ramp the commanded position up.
        let mut cmd_pos = base;
        for _ in 0..200 {
            bus.with_image_mut(|img| axis.tick(img, &drive, dt, None));
            pollster::block_on(bus.exchange()).unwrap();
            cmd_pos = axis.status().cmd_pos;
        }

        // The overlay shifted commanded position by ~the delta (jerk-limited ramp
        // has completed after 200 * 2ms = 0.4s for a 10-unit move at these limits).
        assert!(
            (cmd_pos - (base + 10.0)).abs() < 1e-3,
            "superimpose should shift commanded pos by ~+10 (base {base}, got {cmd_pos})"
        );
    }

    #[test]
    fn superimpose_moves_then_trends_toward_delta() {
        // Fix 1 (early ramp): the overlay must already be nonzero a few cycles in
        // and trend monotonically toward the delta (it must not stay at base).
        let (mut axis, mut bus) = enabled_axis();
        let drive = MockDrive::for_axis(0);
        let dt = 0.002;
        let base = axis.status().cmd_pos;

        let cmd = proto::AxisCommand {
            axis_id: 0,
            token: 7,
            kind: proto::CommandKind::Superimpose,
            params: cmd_params(10.0, 50.0, 500.0, 5000.0),
        };
        axis.apply_command(&cmd, None);

        let mut last = base;
        let mut moved = false;
        for _ in 0..10 {
            bus.with_image_mut(|img| axis.tick(img, &drive, dt, None));
            pollster::block_on(bus.exchange()).unwrap();
            let now = axis.status().cmd_pos;
            if now > base + 1e-9 {
                moved = true;
            }
            assert!(now >= last - 1e-9, "overlay ramps monotonically up");
            last = now;
        }
        assert!(
            moved,
            "commanded pos must move off base while overlay ramps"
        );
        assert!(
            last < base + 10.0,
            "still ramping toward delta, not overshot"
        );
    }

    #[test]
    fn faulted_axis_rejects_move_with_error() {
        // Fix 2: a motion command issued while the axis is latched in ErrorStop
        // must be rejected (token Error), must NOT mutate motion, and must NOT
        // clear the latch. Only Power/Reset are accepted.
        let (mut axis, _bus) = enabled_axis();

        // Latch the safe-state reaction (as the engaged-fault path would).
        axis.force_error_stop();
        let motion_before = axis.commanded_state();

        let mv = proto::AxisCommand {
            axis_id: 0,
            token: 1,
            kind: proto::CommandKind::MoveVelocity,
            params: cmd_params(0.0, 25.0, 100.0, 0.0),
        };
        axis.apply_command(&mv, None);

        assert_eq!(
            axis.token_state_for_test(),
            proto::TokenState::Error,
            "move on a faulted axis is rejected with Error, not Active"
        );
        assert!(axis.error_stop_for_test(), "fault latch must remain set");
        assert_eq!(
            axis.commanded_state(),
            motion_before,
            "rejected command must not mutate the active motion"
        );

        // Reset clears the latch and re-arms toward Enabled.
        let reset = proto::AxisCommand {
            axis_id: 0,
            token: 2,
            kind: proto::CommandKind::Reset,
            params: proto::CommandParams::default(),
        };
        axis.apply_command(&reset, None);
        assert!(!axis.error_stop_for_test(), "Reset clears the fault latch");
        assert_eq!(
            axis.token_state_for_test(),
            proto::TokenState::Active,
            "Reset is accepted (Active) on a faulted axis"
        );
    }
}
