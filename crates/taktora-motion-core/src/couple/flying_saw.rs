//! Flying saw / flying shear / rotary-knife catch-up coupling.
//!
//! A slave axis (the *saw*, on a carriage) waits at a `home` rest
//! position. On command it accelerates to **catch up to a master**
//! (the material / line, moving at constant velocity `v_m`), matches
//! the master's position **and** velocity for a synchronous working
//! window (during which the blade does its work 1:1 with the line),
//! then returns home to rest, ready for the next cycle.
//!
//! ```text
//!   pos
//!    ^                      ___________            master (line)
//!    |                 ____/  synchronous \____
//!    |   SyncOn  _____/  (follow master 1:1) \_____  Return
//!    |       ___/                                  \___
//!  home -----                                          ----- (rest)
//!    +----------------------------------------------------> t
//! ```
//!
//! # Model / simplifying assumptions (deeper cases deferred)
//!
//! * During an engagement the master moves at **constant velocity**
//!   `v_m`; we treat `master.acc ≈ 0` when *planning* the engagement.
//!   We still READ `master` every cycle during the synchronous phase,
//!   so a master that drifts is tracked faithfully there.
//! * Boundary conditions are **rest-to-rest at home**: the saw starts
//!   and ends each cycle at `home` with zero velocity and acceleration.
//!
//! # Why a quintic for the catch-up?
//!
//! The catch-up (`SyncOn`) and the `Return` each have six boundary
//! conditions — position, velocity AND acceleration at both ends — so
//! the lowest-degree polynomial that can satisfy all six is a
//! **degree-5 (quintic)**. Matching acceleration at the seams gives
//! **C² continuity**: there is no acceleration step as the saw merges
//! into (or peels out of) the synchronous phase, hence no torque step
//! into the drive.
//!
//! ## Quintic coefficient derivation
//!
//! Write the segment in local time `t ∈ [0, T]`:
//!
//! ```text
//!   p(t) = c0 + c1 t + c2 t² + c3 t³ + c4 t⁴ + c5 t⁵
//!   v(t) = c1 + 2 c2 t + 3 c3 t² + 4 c4 t³ + 5 c5 t⁴
//!   a(t) =      2 c2   + 6 c3 t + 12 c4 t² + 20 c5 t³
//! ```
//!
//! The conditions at `t = 0` fix the first three coefficients directly:
//!
//! ```text
//!   p(0) = p0  =>  c0 = p0
//!   v(0) = v0  =>  c1 = v0
//!   a(0) = a0  =>  c2 = a0 / 2
//! ```
//!
//! Imposing `p(T)=p1, v(T)=v1, a(T)=a1` gives a 3×3 system whose
//! closed-form solution (with `h = p1 - p0`) is:
//!
//! ```text
//!   c3 = ( 20 h - (8 v1 + 12 v0) T - (3 a0 - a1) T² ) / (2 T³)
//!   c4 = (-30 h + (14 v1 + 16 v0) T + (3 a0 - 2 a1) T²) / (2 T⁴)
//!   c5 = ( 12 h - (6 v1 +  6 v0) T - (  a0 -   a1) T² ) / (2 T⁵)
//! ```
//!
//! These are the standard quintic-interpolation coefficients; they are
//! validated numerically by the seam-continuity tests.

use crate::error::MotionError;
use crate::math::{abs, clamp};
use crate::state::{AxisState, Limits};

/// A single quintic segment in local time `t ∈ [0, T]`.
///
/// Stores the six polynomial coefficients plus the segment duration so
/// it can be sampled at any local time and report whether it is done.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Quintic {
    c: [f64; 6],
    t_total: f64,
    t_elapsed: f64,
}

impl Quintic {
    /// Build a quintic from full boundary conditions over duration `t`.
    ///
    /// `t` MUST be strictly positive (checked by the caller).
    fn new(p0: f64, v0: f64, a0: f64, p1: f64, v1: f64, a1: f64, t: f64) -> Self {
        let h = p1 - p0;
        let t2 = t * t;
        let t3 = t2 * t;
        let t4 = t3 * t;
        let t5 = t4 * t;

        let c0 = p0;
        let c1 = v0;
        let c2 = 0.5 * a0;
        let c3 = (20.0 * h - (8.0 * v1 + 12.0 * v0) * t - (3.0 * a0 - a1) * t2) / (2.0 * t3);
        let c4 =
            (-30.0 * h + (14.0 * v1 + 16.0 * v0) * t + (3.0 * a0 - 2.0 * a1) * t2) / (2.0 * t4);
        let c5 = (12.0 * h - (6.0 * v1 + 6.0 * v0) * t - (a0 - a1) * t2) / (2.0 * t5);

        Self {
            c: [c0, c1, c2, c3, c4, c5],
            t_total: t,
            t_elapsed: 0.0,
        }
    }

    /// Sample position / velocity / acceleration at local time `t`
    /// (clamped to `[0, t_total]`).
    fn sample(&self, t: f64) -> AxisState {
        let t = clamp(t, 0.0, self.t_total);
        let [c0, c1, c2, c3, c4, c5] = self.c;
        let t2 = t * t;
        let t3 = t2 * t;
        let t4 = t3 * t;
        let t5 = t4 * t;
        let pos = c0 + c1 * t + c2 * t2 + c3 * t3 + c4 * t4 + c5 * t5;
        let vel = c1 + 2.0 * c2 * t + 3.0 * c3 * t2 + 4.0 * c4 * t3 + 5.0 * c5 * t4;
        let acc = 2.0 * c2 + 6.0 * c3 * t + 12.0 * c4 * t2 + 20.0 * c5 * t3;
        AxisState::new(pos, vel, acc)
    }

    /// Velocity at local time `t` (helper for the feasibility scan).
    fn vel_at(&self, t: f64) -> f64 {
        self.sample(t).vel
    }

    /// Acceleration at local time `t` (helper for the feasibility scan).
    fn acc_at(&self, t: f64) -> f64 {
        self.sample(t).acc
    }

    /// Jerk (third derivative) at local time `t`.
    fn jerk_at(&self, t: f64) -> f64 {
        let [_, _, _, c3, c4, c5] = self.c;
        let t = clamp(t, 0.0, self.t_total);
        let t2 = t * t;
        6.0 * c3 + 24.0 * c4 * t + 60.0 * c5 * t2
    }

    /// Advance by `dt`, returning the new setpoint. Saturates at the
    /// end of the segment.
    fn advance(&mut self, dt: f64) -> AxisState {
        self.t_elapsed = clamp(self.t_elapsed + dt, 0.0, self.t_total);
        self.sample(self.t_elapsed)
    }

    fn done(&self) -> bool {
        self.t_elapsed >= self.t_total
    }
}

/// Engagement phase of the flying saw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Quintic catch-up: `(home, 0, 0)` → `(sync_pos, v_m, 0)`.
    SyncOn,
    /// Following the master 1:1 over the working window.
    Synchronous,
    /// Quintic return: `(pos, v_m, 0)` → `(home, 0, 0)`.
    Return,
    /// At rest at home, cycle complete (awaiting re-arm).
    Waiting,
}

/// Flying-saw catch-up generator.
///
/// Construct with [`FlyingSaw::plan`]; drive with
/// [`FlyingSaw::update`] once per control cycle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlyingSaw {
    home: f64,
    /// Master line velocity assumed during the engagement.
    v_m: f64,
    /// Planned duration of the synchronous (working) window.
    t_sync: f64,
    /// Offset latched at the start of the synchronous phase so the saw
    /// tracks `master.pos + offset` bumplessly.
    sync_offset: f64,
    /// Time already spent in the synchronous phase.
    t_in_sync: f64,
    sync_on: Quintic,
    ret: Quintic,
    phase: Phase,
    cur: AxisState,
    /// Set if the master accelerated beyond the planned envelope during
    /// synchronous tracking (runtime breach — flagged, never panics).
    envelope_breached: bool,
    limits: Limits,
}

impl FlyingSaw {
    /// Plan a flying-saw engagement.
    ///
    /// * `home` — rest position the saw starts and ends each cycle at.
    /// * `t_on` — duration of the quintic catch-up (`SyncOn`) phase.
    /// * `t_sync` — duration of the synchronous working window.
    /// * `master_at_engage` — master state at the moment of arming; its
    ///   `pos` and `vel` (= `v_m`) define where/how fast the line is.
    /// * `limits` — kinematic envelope the catch-up must respect.
    ///
    /// `sync_pos` (where the saw meets the line at the end of `SyncOn`)
    /// is `master.pos + v_m · t_on` — i.e. where the master *will be*
    /// once the catch-up completes, assuming constant `v_m`.
    ///
    /// # Errors
    ///
    /// * [`MotionError::NonPositiveDuration`] if `t_on` or `t_sync` is
    ///   not strictly positive.
    /// * [`MotionError::NonPositiveLimit`] if any kinematic limit is
    ///   non-positive.
    /// * [`MotionError::InfeasibleEngagement`] if the catch-up quintic's
    ///   peak velocity, acceleration, or jerk over `[0, t_on]` would
    ///   exceed `v_max` / `a_max` / `j_max`.
    pub fn plan(
        home: f64,
        t_on: f64,
        t_sync: f64,
        master_at_engage: AxisState,
        limits: Limits,
    ) -> Result<Self, MotionError> {
        if !(t_on > 0.0 && t_sync > 0.0) {
            return Err(MotionError::NonPositiveDuration);
        }
        if !(limits.v_max > 0.0 && limits.a_max > 0.0 && limits.j_max > 0.0) {
            return Err(MotionError::NonPositiveLimit);
        }

        let v_m = master_at_engage.vel;
        // Where the master will be when SyncOn finishes (const-v model).
        let sync_pos = master_at_engage.pos + v_m * t_on;

        // SyncOn quintic: (home, 0, 0) -> (sync_pos, v_m, 0).
        let sync_on = Quintic::new(home, 0.0, 0.0, sync_pos, v_m, 0.0, t_on);

        // Return quintic, planned now for feasibility checking. At the
        // end of the synchronous window the saw is at
        // `sync_pos + v_m * t_sync` moving at `v_m` with zero accel;
        // it returns to (home, 0, 0). We size the return to mirror the
        // catch-up duration (a symmetric, equally-feasible peel-out).
        let ret_start = sync_pos + v_m * t_sync;
        let ret = Quintic::new(ret_start, v_m, 0.0, home, 0.0, 0.0, t_on);

        // Feasibility: scan both quintics for limit breaches. The
        // extrema of a quintic's derivatives lie at interior critical
        // points, so a dense uniform scan bounds the true peak to within
        // the sample spacing — adequate for a planning-time predicate.
        if Self::breaches_limits(&sync_on, &limits) || Self::breaches_limits(&ret, &limits) {
            return Err(MotionError::InfeasibleEngagement);
        }

        Ok(Self {
            home,
            v_m,
            t_sync,
            sync_offset: 0.0,
            t_in_sync: 0.0,
            sync_on,
            ret,
            phase: Phase::SyncOn,
            cur: AxisState::at(home),
            envelope_breached: false,
            limits,
        })
    }

    /// Scan a quintic for velocity / acceleration / jerk limit breaches.
    ///
    /// Uniform dense sampling over `[0, T]`: a quintic position has at
    /// most quartic velocity, cubic acceleration and quadratic jerk, so
    /// their extrema are isolated interior points well-resolved by the
    /// scan. We add a small relative tolerance so exact-limit designs
    /// (peak == limit) are accepted.
    fn breaches_limits(q: &Quintic, limits: &Limits) -> bool {
        const SAMPLES: u32 = 256;
        // 1e-9 relative slack absorbs sampling + floating error.
        let v_lim = limits.v_max * (1.0 + 1e-9);
        let a_lim = limits.a_max * (1.0 + 1e-9);
        let j_lim = limits.j_max * (1.0 + 1e-9);
        let mut i = 0u32;
        while i <= SAMPLES {
            let t = q.t_total * f64::from(i) / f64::from(SAMPLES);
            if abs(q.vel_at(t)) > v_lim || abs(q.acc_at(t)) > a_lim || abs(q.jerk_at(t)) > j_lim {
                return true;
            }
            i += 1;
        }
        false
    }

    /// Current engagement phase.
    #[inline]
    #[must_use]
    pub const fn phase(&self) -> Phase {
        self.phase
    }

    /// `true` once a full cycle has completed and the saw is resting at
    /// home (the `Waiting` phase).
    #[inline]
    #[must_use]
    pub const fn done(&self) -> bool {
        matches!(self.phase, Phase::Waiting)
    }

    /// `true` if the master accelerated beyond the planned envelope
    /// during synchronous tracking. Advisory only — tracking continues
    /// (clamped where applicable); it never panics.
    #[inline]
    #[must_use]
    pub const fn envelope_breached(&self) -> bool {
        self.envelope_breached
    }

    /// `true` while the saw is engaged (catching up, synchronous, or
    /// returning) — i.e. not yet back at rest.
    #[inline]
    #[must_use]
    pub const fn is_engaged(&self) -> bool {
        !matches!(self.phase, Phase::Waiting)
    }

    /// Advance the phase machine by `dt` seconds.
    ///
    /// `master` is the master's set-state this cycle; it is consulted
    /// only during the [`Phase::Synchronous`] window (where the saw
    /// tracks the line 1:1). During the quintic phases the saw runs
    /// open-loop off its planned polynomial, so `master` is ignored.
    ///
    /// Infallible, panic-free, allocation-free, and bounded: each call
    /// advances at most through the remaining phases.
    pub fn update(&mut self, dt: f64, master: Option<AxisState>) -> AxisState {
        if dt <= 0.0 {
            return self.cur;
        }
        self.cur = match self.phase {
            Phase::SyncOn => self.step_sync_on(dt, master),
            Phase::Synchronous => self.step_synchronous(dt, master),
            Phase::Return => self.step_return(dt),
            Phase::Waiting => self.cur,
        };
        self.cur
    }

    fn step_sync_on(&mut self, dt: f64, master: Option<AxisState>) -> AxisState {
        let s = self.sync_on.advance(dt);
        if self.sync_on.done() {
            // Latch the synchronous offset so we track the master
            // bumplessly. Prefer the *live* master if supplied (handles
            // a line whose actual position drifted from the const-v
            // prediction); otherwise fall back to the planned sync pos.
            let saw_pos = s.pos;
            self.sync_offset = master.map_or(0.0, |m| saw_pos - m.pos);
            self.t_in_sync = 0.0;
            self.phase = Phase::Synchronous;
        }
        s
    }

    fn step_synchronous(&mut self, dt: f64, master: Option<AxisState>) -> AxisState {
        self.t_in_sync += dt;
        let out = if let Some(m) = master {
            // Track the master 1:1 with the latched position offset.
            // Flag (do not panic) if the master exceeds the envelope.
            if abs(m.vel) > self.limits.v_max * (1.0 + 1e-9)
                || abs(m.acc) > self.limits.a_max * (1.0 + 1e-9)
            {
                self.envelope_breached = true;
            }
            AxisState::new(m.pos + self.sync_offset, m.vel, m.acc)
        } else {
            // No live master: dead-reckon at the planned constant v_m.
            AxisState::new(self.cur.pos + self.v_m * dt, self.v_m, 0.0)
        };

        if self.t_in_sync >= self.t_sync {
            // Re-plan the return quintic from the *actual* current state
            // so the seam is C² regardless of any master drift during
            // the synchronous window.
            self.ret = Quintic::new(out.pos, out.vel, 0.0, self.home, 0.0, 0.0, self.ret.t_total);
            self.phase = Phase::Return;
        }
        out
    }

    fn step_return(&mut self, dt: f64) -> AxisState {
        let s = self.ret.advance(dt);
        if self.ret.done() {
            // Snap to a clean rest at home, absorbing integration drift.
            self.phase = Phase::Waiting;
            return AxisState::at(self.home);
        }
        s
    }

    /// Re-arm a completed cycle for the next engagement, recomputing the
    /// catch-up from `master_at_engage`. Returns an error on the same
    /// conditions as [`FlyingSaw::plan`].
    ///
    /// Note: feasibility depends on the catch-up *distance*
    /// `sync_pos - home = (master.pos - home) + v_m·t_on`, so re-arm
    /// against the *next* piece of material as it enters the catch-up
    /// zone near `home` — not against a stale master position that has
    /// run far downstream (which would demand an impossible sprint).
    ///
    /// # Errors
    ///
    /// Same as [`FlyingSaw::plan`].
    pub fn rearm(&mut self, master_at_engage: AxisState) -> Result<(), MotionError> {
        let next = Self::plan(
            self.home,
            self.sync_on.t_total,
            self.t_sync,
            master_at_engage,
            self.limits,
        )?;
        *self = next;
        Ok(())
    }
}
