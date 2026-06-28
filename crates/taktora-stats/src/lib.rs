//! Allocation-free `no_std` statistics primitives for taktora telemetry
//! (`ADR_0062` / `BB_0053`).
//!
//! Two single-writer data structures:
//!
//! * [`RollingHistogram`] — sliding-window percentile estimator over
//!   sub-octave (mantissa-subdivided `log2`) buckets, aged a whole segment
//!   at a time (the snapshot-ring of `ADR_0060`). Backs `REQ_0100` /
//!   `REQ_0101` / `REQ_0852`.
//! * [`MinMaxDeque`] — exact windowed min/max via a monotonic deque.
//!   Backs `REQ_0105`.
//!
//! A composite built on these two primitives:
//!
//! * [`CycleStatsCore`] — per-quantity bundle wrapping one
//!   `RollingHistogram` and one `MinMaxDeque` for a single nanosecond
//!   measurement (e.g. connector wire-round duration). `BB_0054`.
//!
//! Both primitives are `&mut`-based and contain no `unsafe`, no allocation, and no
//! interior mutability. Concurrent (lossy) reads for a stats snapshot are
//! the consumer's responsibility: recompute and publish derived values to
//! relaxed atomics after each `&mut` update (see the executor / connector
//! integration plans).
#![cfg_attr(not(test), no_std)]

/// Worst-case relative error of a histogram percentile estimate, as a
/// whole-number percent.
///
/// The percentile path ([`RollingHistogram::percentile`]) reports the
/// geometric centroid of a *sub-octave* bucket ([`bucket_midpoint`]). With
/// `M` mantissa sub-buckets per octave the widest bucket spans a ratio of
/// `1 + 1/M`, so the centroid is within `√(1 + 1/M) − 1` of any value the
/// bucket can hold — ≤ 1 % across the required 100 ns … 10 s range
/// (`REQ_0852`, verified by `TEST_0868`). Consumers that need exact figures
/// (SLA thresholds, regression gates) must still use the exact `min`/`max`
/// extremes, not the percentiles.
pub const PERCENTILE_MAX_REL_ERR_PCT: u8 = 1;

mod connector;
mod cyclestats;
mod execcycle;
mod histogram;
mod minmax;

pub use connector::{ConnectorCycleSnapshot, ConnectorCycleStats};
pub use cyclestats::CycleStatsCore;
pub use execcycle::{ExecutorCycleSnapshot, ExecutorCycleStats};
pub use histogram::{BUCKETS, RollingHistogram, bucket_index, bucket_lower, bucket_midpoint};
pub use minmax::MinMaxDeque;
