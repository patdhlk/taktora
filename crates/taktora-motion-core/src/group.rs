//! [`Axis`] and [`AxisGroup`] — the per-cycle tick contract.

use crate::couple::Gear;
use crate::math;
use crate::motion::Motion;
use crate::state::{AxisState, AxisStatus};

/// A single axis: its active generator, optional modulo period, optional master
/// coupling, and last published set-state.
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
    /// Each axis reads its master's already-updated, modulo-wrapped state, runs
    /// its generator, then has the per-axis modulo wrap applied once — keeping
    /// rotary rollover in exactly one place. Bounded, allocation-free,
    /// panic-free.
    pub fn tick(&mut self, dt: f64) {
        for k in 0..N {
            let i = self.order[k] as usize;
            let master = self.axes[i].master_idx.map(|m| self.axes[m as usize].state);
            let mut next = self.axes[i].motion.update(dt, master);
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
