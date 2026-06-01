//! Error type for *constructing* motion commands.
//!
//! The hot path ([`Motion::update`](crate::Motion::update) /
//! [`AxisGroup::tick`](crate::AxisGroup::tick)) is **infallible** — it never
//! allocates, never panics, and never returns an error. Validation happens once
//! at command-construction time and surfaces here.

/// Why a motion command could not be constructed.
///
/// Returned by profile/coupling constructors (e.g.
/// [`TrapState::plan`](crate::profile::TrapState::plan)). Steady-state ticking
/// cannot produce these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MotionError {
    /// A kinematic limit (`v_max`, `a_max`, or `j_max`) was zero or negative.
    NonPositiveLimit,
    /// The requested target position lies outside the configured soft limits.
    TargetOutOfLimits,
    /// A coupling referenced a master axis index that is out of range or is not
    /// ordered upstream of the slave.
    InvalidMaster,
    /// A planned duration (e.g. a flying-saw sync or synchronous window) was
    /// zero or negative.
    NonPositiveDuration,
    /// A flying-saw engagement cannot be executed within the supplied kinematic
    /// limits — the catch-up/return quintic's peak velocity, acceleration, or
    /// jerk would exceed `v_max` / `a_max` / `j_max`.
    InfeasibleEngagement,
}

impl core::fmt::Display for MotionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let msg = match self {
            Self::NonPositiveLimit => "kinematic limit must be strictly positive",
            Self::TargetOutOfLimits => "target position is outside the soft limits",
            Self::InvalidMaster => "coupling master index is invalid or not upstream",
            Self::NonPositiveDuration => "planned duration must be strictly positive",
            Self::InfeasibleEngagement => "engagement is infeasible within the kinematic limits",
        };
        f.write_str(msg)
    }
}

impl core::error::Error for MotionError {}
