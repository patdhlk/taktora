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
use taktora_motion_core::{Axis, AxisState as CoreAxisState, Motion};
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
    /// Active correlation token (full lifecycle mapping is Task 12).
    last_token: proto::Token,
    token_state: proto::TokenState,
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
            // coupled master's commanded state (`None` if uncoupled).
            let next = self.arm.motion.update(dt, master);
            self.arm.state = next;
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

    /// Replace the active trajectory generator (Task 12 command mapping).
    #[expect(dead_code, reason = "wired by command mapping in Task 12")]
    pub(crate) const fn set_motion(&mut self, motion: Motion) {
        self.arm.motion = motion;
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
}
