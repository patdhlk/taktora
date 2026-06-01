//! Axis kinematic state, limits, and PLCopen-flavored status.

/// The commanded ("set") kinematic state of an axis for one cycle.
///
/// All quantities are `f64` in engineering units: `pos` in user units,
/// `vel` in units/s, `acc` in units/s². This is the *commanded* state produced
/// by a [`Motion`](crate::Motion) generator — not feedback.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AxisState {
    /// Commanded position (user units). May be modulo-wrapped by the owning
    /// [`Axis`](crate::Axis) — see [`AxisGroup::tick`](crate::AxisGroup::tick).
    pub pos: f64,
    /// Commanded velocity (units/s).
    pub vel: f64,
    /// Commanded acceleration (units/s²).
    pub acc: f64,
}

impl AxisState {
    /// The origin at rest: position, velocity, and acceleration all zero.
    pub const ZERO: Self = Self {
        pos: 0.0,
        vel: 0.0,
        acc: 0.0,
    };

    /// A state at rest at `pos` (zero velocity and acceleration).
    #[inline]
    #[must_use]
    pub const fn at(pos: f64) -> Self {
        Self {
            pos,
            vel: 0.0,
            acc: 0.0,
        }
    }

    /// A full kinematic state from explicit position, velocity, and acceleration.
    #[inline]
    #[must_use]
    pub const fn new(pos: f64, vel: f64, acc: f64) -> Self {
        Self { pos, vel, acc }
    }
}

impl Default for AxisState {
    fn default() -> Self {
        Self::ZERO
    }
}

/// Kinematic limits for an axis.
///
/// `j_max` (jerk) is carried for forward-compatibility with the jerk-limited
/// S-curve profile; the v1 [`Trapezoid`](crate::profile::TrapState) profile
/// uses only `v_max` and `a_max`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    /// Maximum velocity magnitude (units/s). Must be strictly positive.
    pub v_max: f64,
    /// Maximum acceleration magnitude (units/s²). Must be strictly positive.
    pub a_max: f64,
    /// Maximum jerk magnitude (units/s³). Must be strictly positive when used.
    pub j_max: f64,
    /// Lower soft position limit (user units).
    pub pos_min: f64,
    /// Upper soft position limit (user units).
    pub pos_max: f64,
}

impl Limits {
    /// Construct limits, validating that `v_max`/`a_max` are strictly positive
    /// and `pos_min <= pos_max`.
    ///
    /// # Errors
    ///
    /// Returns [`MotionError::NonPositiveLimit`](crate::MotionError::NonPositiveLimit)
    /// if `v_max` or `a_max` is not strictly positive, or if `pos_min > pos_max`.
    pub fn new(
        v_max: f64,
        a_max: f64,
        j_max: f64,
        pos_min: f64,
        pos_max: f64,
    ) -> Result<Self, crate::MotionError> {
        if !crate::math::is_positive(v_max) || !crate::math::is_positive(a_max) || pos_min > pos_max
        {
            return Err(crate::MotionError::NonPositiveLimit);
        }
        Ok(Self {
            v_max,
            a_max,
            j_max,
            pos_min,
            pos_max,
        })
    }

    /// Kinematic-only limits (`v_max`/`a_max`/`j_max`) with unbounded soft
    /// position limits. Convenience for generators that do not consult the
    /// position window (e.g. the flying saw); validation of positivity is the
    /// generator's `plan` responsibility.
    #[inline]
    #[must_use]
    pub const fn kinematic(v_max: f64, a_max: f64, j_max: f64) -> Self {
        Self {
            v_max,
            a_max,
            j_max,
            pos_min: f64::NEG_INFINITY,
            pos_max: f64::INFINITY,
        }
    }
}

/// PLCopen-flavored axis status.
///
/// This is a **read model** derived from the active [`Motion`](crate::Motion)
/// generator (see [`Axis::status`](crate::Axis::status)), supplying the
/// standardized vocabulary. The full `PLCopen` transition state machine
/// (`Power`/`Reset`/`Halt` gating, buffered moves) is intentionally **not yet**
/// modeled here — it is a deferred design decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AxisStatus {
    /// Power stage off / no motion authority.
    Disabled,
    /// Powered and at rest, ready to accept a move.
    Standstill,
    /// Executing a finite (point-to-point) move.
    DiscreteMotion,
    /// Executing an open-ended (e.g. velocity) move.
    ContinuousMotion,
    /// Following a master via electronic coupling.
    SynchronizedMotion,
    /// Latched fault; requires reset before motion resumes.
    ErrorStop,
}
