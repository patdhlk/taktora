//! Constant-velocity ("jog") move with a bounded acceleration ramp.
//!
//! Drives toward a target velocity, ramping at `a_max`, integrating position
//! along the way. This is the simplest *continuous* generator and is what a
//! [virtual master](crate::master) typically runs.

use crate::math;
use crate::state::AxisState;

/// A velocity move: ramp to `target_vel` at `a_max`, then hold.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VelocityMove {
    pos: f64,
    vel: f64,
    target_vel: f64,
    a_max: f64,
}

impl VelocityMove {
    /// Start a velocity move from `start`, ramping toward `target_vel` at
    /// `a_max` (units/s²). `a_max` is taken as a magnitude.
    #[inline]
    #[must_use]
    pub fn new(start: AxisState, target_vel: f64, a_max: f64) -> Self {
        Self {
            pos: start.pos,
            vel: start.vel,
            target_vel,
            a_max: math::abs(a_max),
        }
    }

    /// Change the commanded velocity mid-move (the bounded analogue of
    /// `PLCopen`'s continuous-update). The ramp continues at `a_max`.
    #[inline]
    pub const fn set_target(&mut self, target_vel: f64) {
        self.target_vel = target_vel;
    }

    /// Advance by `dt` seconds and return the new commanded state.
    /// Bounded, allocation-free, panic-free.
    #[must_use]
    pub fn update(&mut self, dt: f64) -> AxisState {
        if !math::is_positive(dt) {
            return AxisState {
                pos: self.pos,
                vel: self.vel,
                acc: 0.0,
            };
        }
        let v_old = self.vel;
        let dv = self.target_vel - v_old;
        let step = self.a_max * dt;
        let acc = if dv > step {
            self.vel = v_old + step;
            self.a_max
        } else if dv < -step {
            self.vel = v_old - step;
            -self.a_max
        } else {
            self.vel = self.target_vel;
            dv / dt
        };
        // Trapezoidal integration of position over the cycle.
        self.pos += f64::midpoint(v_old, self.vel) * dt;
        AxisState {
            pos: self.pos,
            vel: self.vel,
            acc,
        }
    }
}
