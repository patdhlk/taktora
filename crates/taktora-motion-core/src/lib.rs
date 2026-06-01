//! Allocation-free, `no_std` real-time motion **trajectory core** for taktora.
//!
//! This crate computes commanded axis setpoints as bounded, allocation-free,
//! panic-free functions of `(dt, master)`. It owns no I/O, no threads, and no
//! shared mutable state — the executor/iceoryx2 glue lives in `taktora-motion`.
//!
//! The deployment model is a **setpoint generator** for `CiA` 402 drives in
//! Cyclic Synchronous Position (CSP) mode: this crate produces the commanded
//! position each cycle; the drive closes its own velocity and current loops.
//!
//! # Shape
//!
//! An [`Axis`] owns a [`Motion`] generator. An [`AxisGroup`] ticks a fixed
//! array of axes in a build-time topological order (masters before slaves), so
//! electronic coupling is same-cycle coherent. [`Motion`] is a monomorphized
//! enum — no `Box<dyn>`, no vtable on the hot path.
//!
//! ```
//! use taktora_motion_core::{Axis, AxisGroup, AxisState, Limits, Motion};
//! use taktora_motion_core::profile::VelocityMove;
//! use taktora_motion_core::couple::Gear;
//!
//! // Axis 0: a virtual master jogging at 10 units/s.
//! let master = Axis::new(Motion::Velocity(VelocityMove::new(
//!     AxisState::ZERO,
//!     10.0, // target velocity
//!     50.0, // accel limit
//! )));
//!
//! // Axis 1: geared 2:1 to the master.
//! let slave = Axis::geared(Gear::new(2.0), 0);
//!
//! let mut group = AxisGroup::new([master, slave], [0, 1]);
//! group.tick(0.001);
//! ```
//!
//! # Conventions
//!
//! - `f64` engineering units; increments↔units scaling is the glue's job.
//! - `dt` is seconds, passed per tick; the integrator tolerates a late cycle.
//! - Modulo (endless rotary) wrap is applied once in [`AxisGroup::tick`].

#![no_std]
#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod couple;
pub mod error;
pub mod group;
pub mod master;
pub mod math;
pub mod motion;
pub mod profile;
pub mod state;

pub use error::MotionError;
pub use group::{Axis, AxisGroup};
pub use motion::Motion;
pub use state::{AxisState, AxisStatus, Limits};
