//! Master/slave coupling generators (synchronized motion).
//!
//! Coupled generators read the *master's* set-state for the current cycle.
//! [`AxisGroup::tick`](crate::AxisGroup::tick) guarantees the master is
//! evaluated before its slaves via the build-time topological order, so
//! coupling is same-cycle coherent.

mod cam;
mod flying_saw;
mod gear;

pub use cam::{Cam, CamSegment, CamTable};
pub use flying_saw::{FlyingSaw, Phase};
pub use gear::Gear;
