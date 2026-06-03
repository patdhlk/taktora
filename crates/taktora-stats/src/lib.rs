//! Allocation-free `no_std` statistics primitives for taktora telemetry
//! (`ADR_0062` / `BB_0053`).
//!
//! Two single-writer data structures:
//!
//! * [`RollingHistogram`] — sliding-window percentile estimator over
//!   octave (`log2`) buckets, aged a whole segment at a time (the
//!   snapshot-ring of `ADR_0060`). Backs `REQ_0100` / `REQ_0101`.
//! * [`MinMaxDeque`] — exact windowed min/max via a monotonic deque.
//!   Backs `REQ_0105`.
//!
//! Both are `&mut`-based and contain no `unsafe`, no allocation, and no
//! interior mutability. Concurrent (lossy) reads for a stats snapshot are
//! the consumer's responsibility: recompute and publish derived values to
//! relaxed atomics after each `&mut` update (see the executor / connector
//! integration plans).
#![cfg_attr(not(test), no_std)]

mod cyclestats;
mod histogram;
mod minmax;

pub use cyclestats::CycleStatsCore;
pub use histogram::{BUCKETS, RollingHistogram, bucket_index, bucket_lower};
pub use minmax::MinMaxDeque;
