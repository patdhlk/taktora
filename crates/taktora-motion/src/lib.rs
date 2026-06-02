#![warn(missing_docs)]
//! Cyclic NC task: runs `taktora-motion-core` against `CiA` 402 `CSP` drives
//! over a `CyclicFieldbus` seam. Owns unit-increment scaling, the per-axis
//! power/bumpless/command runtime, the coupling topology, the cyclic step,
//! and a host-side virtual-drive mock (the primary Phase-4 test vehicle).

pub mod axis;
pub mod cycle;
pub mod mock;
pub mod scale;
pub mod topology;
