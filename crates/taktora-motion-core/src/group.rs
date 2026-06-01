//! [`Axis`] and [`AxisGroup`] — the per-cycle tick contract.

use crate::MotionError;
use crate::couple::Gear;
use crate::math;
use crate::motion::Motion;
use crate::profile::SCurveState;
use crate::state::{AxisState, AxisStatus, Limits};

/// A single axis: its active generator, an optional superimposed corrective
/// offset, optional modulo period, optional master coupling, and last published
/// set-state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Axis {
    /// The active setpoint generator.
    pub motion: Motion,
    /// Modulo period (user units) for an endless rotary axis; `None` = linear.
    pub modulo: Option<f64>,
    /// Index (into the owning [`AxisGroup`]) of this axis's master, if coupled.
    pub master_idx: Option<u8>,
    /// Last commanded set-state (modulo-wrapped). Read by downstream slaves.
    pub state: AxisState,
    /// Optional superimposed corrective offset (`PLCopen` `MC_MoveSuperimposed`):
    /// a jerk-limited additive move `0 → Δ` layered on top of `motion` without
    /// interrupting it. Managed via [`Axis::superimpose`] /
    /// [`Axis::clear_superimposed`]; applied in [`AxisGroup::tick`].
    superimposed: Option<SCurveState>,
}

impl Axis {
    /// An uncoupled, linear axis running `motion`, at rest at the origin.
    #[inline]
    #[must_use]
    pub const fn new(motion: Motion) -> Self {
        Self {
            motion,
            modulo: None,
            master_idx: None,
            state: AxisState::ZERO,
            superimposed: None,
        }
    }

    /// A slave axis geared to the master at `master_idx`.
    #[inline]
    #[must_use]
    pub const fn geared(gear: Gear, master_idx: u8) -> Self {
        Self {
            motion: Motion::Gear(gear),
            modulo: None,
            master_idx: Some(master_idx),
            state: AxisState::ZERO,
            superimposed: None,
        }
    }

    /// Make this an endless rotary axis with the given modulo period.
    #[inline]
    #[must_use]
    pub const fn with_modulo(mut self, period: f64) -> Self {
        self.modulo = Some(period);
        self
    }

    /// Couple this axis to the master at `master_idx`.
    #[inline]
    #[must_use]
    pub const fn with_master(mut self, master_idx: u8) -> Self {
        self.master_idx = Some(master_idx);
        self
    }

    /// The axis's PLCopen-flavored status, derived from its generator.
    #[inline]
    #[must_use]
    pub fn status(&self) -> AxisStatus {
        self.motion.status()
    }

    /// Superimpose a jerk-limited corrective offset of `delta` (user units) on
    /// top of the ongoing base motion, without interrupting it — the analogue
    /// of `PLCopen` `MC_MoveSuperimposed`. The offset profiles `0 → delta` under
    /// `limits` and is added to every subsequent tick; once it completes the
    /// (now constant) offset remains applied until [`clear_superimposed`] is
    /// called. Starting a new superimposed move replaces any previous one.
    ///
    /// [`clear_superimposed`]: Self::clear_superimposed
    ///
    /// # Errors
    ///
    /// Propagates [`SCurveState::plan`] errors (non-positive limits).
    pub fn superimpose(&mut self, delta: f64, limits: Limits) -> Result<(), MotionError> {
        // The corrective runs in offset space (0 → delta); position soft-limits
        // do not apply to a relative overlay, so widen them out.
        let lim = Limits {
            pos_min: f64::NEG_INFINITY,
            pos_max: f64::INFINITY,
            ..limits
        };
        self.superimposed = Some(SCurveState::plan(AxisState::ZERO, delta, lim)?);
        Ok(())
    }

    /// Remove the superimposed offset. Note: if a completed offset was holding a
    /// non-zero `delta`, clearing it steps the commanded position back by that
    /// `delta` on the next tick — clear only when that jump is intended.
    #[inline]
    pub const fn clear_superimposed(&mut self) {
        self.superimposed = None;
    }

    /// `true` while a superimposed corrective move is still in progress. Returns
    /// `false` once it completes (even though the constant offset stays applied)
    /// and when none is set.
    #[inline]
    #[must_use]
    pub fn superimposed_active(&self) -> bool {
        self.superimposed.is_some_and(|s| !s.done())
    }
}

impl Default for Axis {
    fn default() -> Self {
        Self::new(Motion::default())
    }
}

/// A fixed-capacity group of `N` axes, ticked in a build-time topological order
/// (masters before slaves) so electronic coupling is same-cycle coherent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AxisGroup<const N: usize> {
    axes: [Axis; N],
    order: [u8; N],
}

impl<const N: usize> AxisGroup<N> {
    /// Build a group from `axes` and a coupling `order` (indices into `axes`,
    /// masters before their slaves). The order is fixed for the group's life.
    #[inline]
    #[must_use]
    pub const fn new(axes: [Axis; N], order: [u8; N]) -> Self {
        Self { axes, order }
    }

    /// Advance every axis by `dt` seconds in topological order.
    ///
    /// For each axis: read the master's already-updated, modulo-wrapped state,
    /// run the base generator, add the superimposed corrective offset (if any),
    /// then apply the per-axis modulo wrap once — keeping rotary rollover in
    /// exactly one place. Bounded, allocation-free, panic-free.
    pub fn tick(&mut self, dt: f64) {
        for k in 0..N {
            let i = self.order[k] as usize;
            let master = self.axes[i].master_idx.map(|m| self.axes[m as usize].state);
            let mut next = self.axes[i].motion.update(dt, master);
            // Superimposed corrective offset (PLCopen MC_MoveSuperimposed):
            // additive on top of the base motion, in offset space.
            if let Some(overlay) = self.axes[i].superimposed.as_mut() {
                let d = overlay.update(dt);
                next.pos += d.pos;
                next.vel += d.vel;
                next.acc += d.acc;
            }
            if let Some(period) = self.axes[i].modulo {
                next.pos = math::rem_euclid(next.pos, period);
            }
            self.axes[i].state = next;
        }
    }

    /// The current set-state of axis `i`.
    #[inline]
    #[must_use]
    pub const fn state(&self, i: usize) -> AxisState {
        self.axes[i].state
    }

    /// Shared access to axis `i` (e.g. to inspect status or swap its motion).
    #[inline]
    #[must_use]
    pub const fn axis(&self, i: usize) -> &Axis {
        &self.axes[i]
    }

    /// Mutable access to axis `i` (e.g. to apply a new motion command).
    #[inline]
    pub const fn axis_mut(&mut self, i: usize) -> &mut Axis {
        &mut self.axes[i]
    }
}
