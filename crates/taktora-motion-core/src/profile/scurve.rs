//! Jerk-limited 7-segment S-curve (double-S) point-to-point profile.
//!
//! Rest-to-rest: zero entry/exit velocity and acceleration. This is the
//! jerk-limited analogue of [`TrapState`](super::TrapState) — the commanded
//! acceleration is itself continuous (C² position), so it does not step-excite
//! drivetrain resonance the way a trapezoid's acceleration discontinuities can.
//!
//! The seven segments are the classic double-S velocity profile (Biagiotti &
//! Melchiorri, *Trajectory Planning for Machines and Robots*, §3.4): jerk
//! `+j, 0, −j` to ramp velocity up to the cruise value, a constant-velocity
//! segment, then `−j, 0, +j` to ramp back to rest. The plan reduces to fewer
//! active segments when `a_max` and/or `v_max` are not reached.
//!
//! On-the-fly retargeting from a non-zero state (the Ruckig *online* use case)
//! is a deferred follow-on.

use crate::MotionError;
use crate::math;
use crate::state::{AxisState, Limits};

const N_SEG: usize = 7;

/// Jerk sign per segment: `+j, 0, −j` (accelerate), cruise, `−j, 0, +j`
/// (decelerate).
const J_SIGN: [f64; N_SEG] = [1.0, 0.0, -1.0, 0.0, -1.0, 0.0, 1.0];

/// A planned jerk-limited S-curve move, advanced by elapsed time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SCurveState {
    /// Per-segment durations (some may be zero when a limit is not reached).
    dur: [f64; N_SEG],
    p0: f64,
    pf: f64,
    dir: f64,
    j_max: f64,
    total: f64,
    t: f64,
}

impl SCurveState {
    /// Plan a rest-to-rest jerk-limited move from `start` to absolute `target`,
    /// respecting `limits` (`v_max`, `a_max`, `j_max`). Entry velocity and
    /// acceleration are assumed zero.
    ///
    /// # Errors
    ///
    /// Returns [`MotionError::NonPositiveLimit`] if any of `v_max`/`a_max`/
    /// `j_max` is not strictly positive, or [`MotionError::TargetOutOfLimits`]
    /// if `target` is outside `[limits.pos_min, limits.pos_max]`.
    pub fn plan(start: AxisState, target: f64, limits: Limits) -> Result<Self, MotionError> {
        if !math::is_positive(limits.v_max)
            || !math::is_positive(limits.a_max)
            || !math::is_positive(limits.j_max)
        {
            return Err(MotionError::NonPositiveLimit);
        }
        if target < limits.pos_min || target > limits.pos_max {
            return Err(MotionError::TargetOutOfLimits);
        }

        let p0 = start.pos;
        let h = target - p0;
        let dist = math::abs(h);
        let dir = math::signum(h);
        let (v, a, j) = (limits.v_max, limits.a_max, limits.j_max);

        if dist <= 0.0 {
            return Ok(Self {
                dur: [0.0; N_SEG],
                p0,
                pf: target,
                dir,
                j_max: j,
                total: 0.0,
                t: 0.0,
            });
        }

        // Step 1 — accel phase that would reach v_max. Does it reach a_max?
        let a_sq = a * a;
        let (mut tj, mut ta) = if v * j >= a_sq {
            (a / j, a / j + v / a) // a_max reached: ramp + const-accel + ramp
        } else {
            let tj = math::sqrt(v / j); // a_max not reached: two jerk segments
            (tj, 2.0 * tj)
        };

        // Step 2 — is v_max actually reached? Cruise time if so.
        let mut tv = dist / v - ta;
        if tv <= 0.0 {
            tv = 0.0;
            // v_max not reached: shrink the peak. First assume a_max is reached.
            let tj_a = a / j;
            let ta_a = tj_a / 2.0 + math::sqrt(tj_a * tj_a / 4.0 + dist / a);
            if ta_a >= 2.0 * tj_a {
                tj = tj_a;
                ta = ta_a;
            } else {
                // Neither limit reached: triangular accel, single cube root.
                tj = math::cbrt(dist / (2.0 * j));
                ta = 2.0 * tj;
            }
        }

        let t_const = ta - 2.0 * tj;
        let t_const = if t_const > 0.0 { t_const } else { 0.0 };
        let dur = [tj, t_const, tj, tv, tj, t_const, tj];
        let total = 4.0 * tj + 2.0 * t_const + tv;

        Ok(Self {
            dur,
            p0,
            pf: target,
            dir,
            j_max: j,
            total,
            t: 0.0,
        })
    }

    /// Total planned duration of the move (seconds).
    #[inline]
    #[must_use]
    pub const fn duration(&self) -> f64 {
        self.total
    }

    /// `true` once the move has reached its target.
    #[inline]
    #[must_use]
    pub fn done(&self) -> bool {
        self.t >= self.total
    }

    /// Advance by `dt` seconds and return the new commanded state.
    /// Bounded (≤7 segment steps), allocation-free, panic-free.
    #[must_use]
    pub fn update(&mut self, dt: f64) -> AxisState {
        if dt > 0.0 {
            self.t += dt;
        }
        if self.total <= 0.0 || self.t >= self.total {
            return AxisState::at(self.pf);
        }

        // Walk segments in "magnitude space" (motion in +dir; velocity stays
        // >= 0, acceleration swings negative during decel), carrying the state
        // forward to the segment that contains `t`.
        let t = self.t;
        let (mut p, mut vel, mut acc) = (0.0_f64, 0.0_f64, 0.0_f64);
        let mut seg_start = 0.0_f64;
        for (i, (&d, &js)) in self.dur.iter().zip(J_SIGN.iter()).enumerate() {
            let j = js * self.j_max;
            let in_this = t <= seg_start + d || i == N_SEG - 1;
            let tau = if in_this { t - seg_start } else { d };

            let p_t = p + vel * tau + 0.5 * acc * tau * tau + (j * tau * tau * tau) / 6.0;
            let v_t = vel + acc * tau + 0.5 * j * tau * tau;
            let a_t = acc + j * tau;

            if in_this {
                return AxisState {
                    pos: self.p0 + self.dir * p_t,
                    vel: self.dir * v_t,
                    acc: self.dir * a_t,
                };
            }
            p = p_t;
            vel = v_t;
            acc = a_t;
            seg_start += d;
        }
        AxisState::at(self.pf)
    }
}
