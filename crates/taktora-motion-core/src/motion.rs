//! The active setpoint generator for an axis.

use crate::couple::{Cam, FlyingSaw, Gear, Phase};
use crate::profile::{SCurveState, TrapState, VelocityMove};
use crate::state::{AxisState, AxisStatus};

/// The active motion generator for an axis.
///
/// Monomorphized — there is **no `Box<dyn>` and no vtable** on the hot path, so
/// [`update`](Self::update) is bounded and allocation-free. Each arm carries
/// its own internal state and produces an absolute [`AxisState`] each cycle.
///
/// Superimposed motion is *not* an arm here — it cannot be (a `Motion` holding
/// a `Motion` would need `Box`): it is realized as an additive corrective
/// overlay on the [`Axis`](crate::Axis) itself (see
/// [`Axis::superimpose`](crate::Axis::superimpose)). The
/// enum is `#[non_exhaustive]` so future generators slot in without a breaking
/// change.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum Motion {
    /// Hold the given position at rest.
    Idle(f64),
    /// Constant-velocity jog with a bounded acceleration ramp.
    Velocity(VelocityMove),
    /// Trapezoidal point-to-point move.
    Trapezoid(TrapState),
    /// Jerk-limited (S-curve) point-to-point move.
    SCurve(SCurveState),
    /// Electronic gearing to a master axis.
    Gear(Gear),
    /// Flying-saw catch-up: quintic sync-on, synchronous tracking, return.
    FlyingSaw(FlyingSaw),
    /// Electronic camming: slave position is a piecewise-quintic function of
    /// the master position.
    Cam(Cam),
}

impl Motion {
    /// Advance the generator by `dt` seconds and return the new commanded
    /// set-state. `master` is the upstream axis's set-state for this cycle
    /// (`None` for uncoupled generators). Bounded, allocation-free, panic-free.
    #[must_use]
    pub fn update(&mut self, dt: f64, master: Option<AxisState>) -> AxisState {
        match self {
            Self::Idle(pos) => AxisState::at(*pos),
            Self::Velocity(v) => v.update(dt),
            Self::Trapezoid(t) => t.update(dt),
            Self::SCurve(s) => s.update(dt),
            Self::Gear(g) => g.update(master),
            Self::FlyingSaw(f) => f.update(dt, master),
            Self::Cam(c) => c.update(dt, master),
        }
    }

    /// Map the active generator to its PLCopen-flavored [`AxisStatus`].
    ///
    /// Note: `Disabled` and `ErrorStop` are not representable here yet — they
    /// belong to the deferred power/fault state machine, not the generator.
    #[must_use]
    pub fn status(&self) -> AxisStatus {
        match self {
            Self::Idle(_) => AxisStatus::Standstill,
            Self::Velocity(_) => AxisStatus::ContinuousMotion,
            Self::Trapezoid(t) => {
                if t.done() {
                    AxisStatus::Standstill
                } else {
                    AxisStatus::DiscreteMotion
                }
            }
            Self::SCurve(s) => {
                if s.done() {
                    AxisStatus::Standstill
                } else {
                    AxisStatus::DiscreteMotion
                }
            }
            // A cam, like a gear, is a master-following synchronized coupling.
            Self::Gear(_) | Self::Cam(_) => AxisStatus::SynchronizedMotion,
            Self::FlyingSaw(f) => match f.phase() {
                Phase::SyncOn | Phase::Synchronous => AxisStatus::SynchronizedMotion,
                Phase::Return => AxisStatus::DiscreteMotion,
                Phase::Waiting => AxisStatus::Standstill,
            },
        }
    }
}

impl Default for Motion {
    fn default() -> Self {
        Self::Idle(0.0)
    }
}
