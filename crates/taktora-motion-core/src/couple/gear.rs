//! Electronic gearing: a slave that follows a master at a fixed ratio.

use crate::state::AxisState;

/// Electronic gear coupling: `slave = ratio · master + offset`.
///
/// The slave's position, velocity, and acceleration are all scaled copies of
/// the master's set-state — same-cycle coherent because [`AxisGroup::tick`]
/// evaluates the master before the slave.
///
/// [`AxisGroup::tick`]: crate::AxisGroup::tick
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gear {
    ratio: f64,
    offset: f64,
}

impl Gear {
    /// A gear with the given ratio and zero position offset.
    #[inline]
    #[must_use]
    pub const fn new(ratio: f64) -> Self {
        Self { ratio, offset: 0.0 }
    }

    /// A gear with a position offset applied after scaling.
    #[inline]
    #[must_use]
    pub const fn with_offset(ratio: f64, offset: f64) -> Self {
        Self { ratio, offset }
    }

    /// Compute the slave set-state from this cycle's `master` state.
    ///
    /// An uncoupled gear (no master available this cycle) holds at the offset.
    #[must_use]
    pub fn update(&self, master: Option<AxisState>) -> AxisState {
        master.map_or_else(
            || AxisState::at(self.offset),
            |m| AxisState {
                pos: self.ratio * m.pos + self.offset,
                vel: self.ratio * m.vel,
                acc: self.ratio * m.acc,
            },
        )
    }
}
