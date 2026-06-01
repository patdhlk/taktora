//! Virtual master helpers.
//!
//! A virtual master is **not a distinct type** — it is an ordinary [`Axis`]
//! with no upstream master, running a profile that slaves couple to. These are
//! thin constructors for the common cases; they are deterministic and
//! replayable, which makes them the natural seam for HIL trace injection.

use crate::group::Axis;
use crate::motion::Motion;
use crate::profile::VelocityMove;
use crate::state::AxisState;

/// A virtual master jogging toward `target_vel` (units/s), ramping at `a_max`.
#[inline]
#[must_use]
pub fn velocity(target_vel: f64, a_max: f64) -> Axis {
    Axis::new(Motion::Velocity(VelocityMove::new(
        AxisState::ZERO,
        target_vel,
        a_max,
    )))
}

/// A virtual master parked at a fixed position (the degenerate, at-rest master).
#[inline]
#[must_use]
pub const fn parked(pos: f64) -> Axis {
    Axis::new(Motion::Idle(pos))
}
