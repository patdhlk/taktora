//! Trapezoidal (and degenerate triangular) point-to-point velocity profile.
//!
//! Velocity-limited and acceleration-limited; jerk is unbounded (acceleration
//! steps at the segment seams). This is the v1 finite-move profile; the
//! jerk-limited S-curve is a deferred follow-on.
//!
//! The plan currently assumes the axis starts **at rest** (`vel ≈ 0`). Blending
//! from a non-zero entry velocity is a later refinement.

use crate::MotionError;
use crate::math;
use crate::state::{AxisState, Limits};

/// A planned trapezoidal move, advanced by elapsed time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrapState {
    p0: f64,
    pf: f64,
    dir: f64,
    a_max: f64,
    v_peak: f64,
    t_acc: f64,
    t_flat: f64,
    t_total: f64,
    t: f64,
}

impl TrapState {
    /// Plan a move from `start` to absolute `target`, respecting `limits`.
    ///
    /// # Errors
    ///
    /// Returns [`MotionError::TargetOutOfLimits`] if `target` is outside
    /// `[limits.pos_min, limits.pos_max]`, or [`MotionError::NonPositiveLimit`]
    /// if `v_max`/`a_max` are not strictly positive.
    pub fn plan(start: AxisState, target: f64, limits: Limits) -> Result<Self, MotionError> {
        if !math::is_positive(limits.v_max) || !math::is_positive(limits.a_max) {
            return Err(MotionError::NonPositiveLimit);
        }
        if target < limits.pos_min || target > limits.pos_max {
            return Err(MotionError::TargetOutOfLimits);
        }

        let p0 = start.pos;
        let delta = target - p0;
        let distance = math::abs(delta);
        let dir = math::signum(delta);
        let a_max = limits.a_max;
        let v_max = limits.v_max;

        if distance <= 0.0 {
            return Ok(Self {
                p0,
                pf: target,
                dir,
                a_max,
                v_peak: 0.0,
                t_acc: 0.0,
                t_flat: 0.0,
                t_total: 0.0,
                t: 0.0,
            });
        }

        // Distance consumed by a full accel ramp to v_max (and back).
        let d_ramp = (v_max * v_max) / (2.0 * a_max);
        let (v_peak, t_acc, t_flat) = if 2.0 * d_ramp <= distance {
            // Trapezoidal: reach v_max, cruise, decelerate.
            let t_acc = v_max / a_max;
            let d_flat = distance - 2.0 * d_ramp;
            (v_max, t_acc, d_flat / v_max)
        } else {
            // Triangular: never reach v_max.
            let v_peak = math::sqrt(a_max * distance);
            (v_peak, v_peak / a_max, 0.0)
        };

        Ok(Self {
            p0,
            pf: target,
            dir,
            a_max,
            v_peak,
            t_acc,
            t_flat,
            t_total: 2.0 * t_acc + t_flat,
            t: 0.0,
        })
    }

    /// `true` once the move has reached its target.
    #[inline]
    #[must_use]
    pub fn done(&self) -> bool {
        self.t >= self.t_total
    }

    /// Advance by `dt` seconds and return the new commanded state.
    /// Bounded, allocation-free, panic-free.
    #[must_use]
    pub fn update(&mut self, dt: f64) -> AxisState {
        if dt > 0.0 {
            self.t += dt;
        }
        if self.t_total <= 0.0 || self.t >= self.t_total {
            return AxisState::at(self.pf);
        }

        let t = self.t;
        let a = self.a_max;
        let (dist, vel, acc) = if t < self.t_acc {
            // Acceleration segment.
            (0.5 * a * t * t, a * t, a)
        } else if t < self.t_acc + self.t_flat {
            // Cruise segment.
            let d_acc = 0.5 * a * self.t_acc * self.t_acc;
            (d_acc + self.v_peak * (t - self.t_acc), self.v_peak, 0.0)
        } else {
            // Deceleration segment.
            let td = t - self.t_acc - self.t_flat;
            let d_acc = 0.5 * a * self.t_acc * self.t_acc;
            let d_flat = self.v_peak * self.t_flat;
            let d_dec = self.v_peak * td - 0.5 * a * td * td;
            (d_acc + d_flat + d_dec, self.v_peak - a * td, -a)
        };

        AxisState {
            pos: self.p0 + self.dir * dist,
            vel: self.dir * vel,
            acc: self.dir * acc,
        }
    }
}
