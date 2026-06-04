//! `ExecutorCycleStats` — executor-side twin of [`ConnectorCycleStats`]
//! (`BB_0050`/`BB_0051`). Folds each cycle's duration, jitter, and lateness
//! into sliding-window aggregates, publishes derived scalars to relaxed
//! atomics for a concurrent pull snapshot (`REQ_0107`). Poison-safe: a
//! faulted cycle (`None` sample) contributes no measurement to the
//! aggregates, but still advances `cycle_index` (`REQ_0107`). Single-writer,
//! allocation-free, `no_std`.
//!
//! Building blocks covered:
//! * `BB_0050` — per-cycle executor duration histogram.
//! * `BB_0051` — per-cycle jitter/lateness tracking.
//! * `REQ_0107` — monotonically advancing `cycle_index` through faults.

use core::sync::atomic::{AtomicU64, Ordering};

use crate::{CycleStatsCore, MinMaxDeque};

/// Borrowed-free value snapshot of the per-executor cycle aggregates.
/// All fields are nanoseconds. `0` denotes "no sample yet".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExecutorCycleSnapshot {
    /// Cycle-duration p50 estimate (ns).
    pub p50_ns: u64,
    /// Cycle-duration p95 estimate (ns).
    pub p95_ns: u64,
    /// Cycle-duration p99 estimate (ns).
    pub p99_ns: u64,
    /// Exact windowed minimum cycle duration (ns).
    pub min_ns: u64,
    /// Exact windowed maximum cycle duration (ns).
    pub max_ns: u64,
    /// Exact windowed maximum jitter magnitude (ns).
    pub max_jitter_ns: u64,
    /// Exact windowed maximum |lateness| magnitude (ns).
    pub max_lateness_ns: u64,
}

/// Per-executor cyclic telemetry aggregator.
///
/// `S` = histogram segment count, `W` = exact min/max window length.
/// Single-writer: [`record_cycle`](Self::record_cycle) takes `&mut self`;
/// [`snapshot`](Self::snapshot) reads published relaxed atomics
/// (per-field tear-free, not coherent across fields).
pub struct ExecutorCycleStats<const S: usize, const W: usize> {
    // Writer-side sliding-window state.
    took: CycleStatsCore<S, W>,
    jitter: MinMaxDeque<W>,
    lateness: MinMaxDeque<W>,
    cycle_index: u64,

    // Published derived scalars (pull path; relaxed atomics).
    pub_p50: AtomicU64,
    pub_p95: AtomicU64,
    pub_p99: AtomicU64,
    pub_min: AtomicU64,
    pub_max: AtomicU64,
    pub_max_jitter: AtomicU64,
    pub_max_lateness: AtomicU64,
}

impl<const S: usize, const W: usize> ExecutorCycleStats<S, W> {
    /// Create an empty aggregator; the cycle-duration histogram window is
    /// approximately `hist_window` samples.
    #[must_use]
    pub fn new(hist_window: u32) -> Self {
        Self {
            took: CycleStatsCore::new(hist_window),
            jitter: MinMaxDeque::new(),
            lateness: MinMaxDeque::new(),
            cycle_index: 0,
            pub_p50: AtomicU64::new(0),
            pub_p95: AtomicU64::new(0),
            pub_p99: AtomicU64::new(0),
            pub_min: AtomicU64::new(0),
            pub_max: AtomicU64::new(0),
            pub_max_jitter: AtomicU64::new(0),
            pub_max_lateness: AtomicU64::new(0),
        }
    }

    /// Number of cycles recorded so far (also the index the next cycle will
    /// be assigned).
    #[must_use]
    pub const fn cycles_recorded(&self) -> u64 {
        self.cycle_index
    }

    /// Fold one cycle. All measurements are `Some(value)` on a healthy cycle
    /// and `None` on a fault — faulted samples are skipped from aggregates
    /// (poison-safe), but `cycle_index` still advances on every call
    /// (`REQ_0107`). Returns the `cycle_index` assigned to this cycle.
    ///
    /// * `took_ns`     — measured cycle duration in nanoseconds (`BB_0050`).
    ///   `u64` (not `u32`): an executor scan-cycle can exceed `u32::MAX` ns
    ///   (~4.3 s) under a long-running or faulted scan, so the wider type is
    ///   required here. The connector's wire-round duration is bounded well
    ///   below that limit and therefore uses `u32`.
    /// * `jitter_ns`   — unsigned jitter magnitude in nanoseconds (`BB_0051`).
    /// * `lateness_ns` — signed deviation from deadline; magnitude is stored.
    pub fn record_cycle(
        &mut self,
        took_ns: Option<u64>,
        jitter_ns: Option<u64>,
        lateness_ns: Option<i64>,
    ) -> u64 {
        let idx = self.cycle_index;
        self.cycle_index += 1;

        if let Some(t) = took_ns {
            self.took.record(t);
            self.pub_p50.store(self.took.p50(), Ordering::Relaxed);
            self.pub_p95.store(self.took.p95(), Ordering::Relaxed);
            self.pub_p99.store(self.took.p99(), Ordering::Relaxed);
            self.pub_min
                .store(self.took.min().unwrap_or(0), Ordering::Relaxed);
            self.pub_max
                .store(self.took.max().unwrap_or(0), Ordering::Relaxed);
        }

        if let Some(j) = jitter_ns {
            self.jitter.record(j);
            self.pub_max_jitter
                .store(self.jitter.max().unwrap_or(0), Ordering::Relaxed);
        }

        if let Some(l) = lateness_ns {
            self.lateness.record(l.unsigned_abs());
            self.pub_max_lateness
                .store(self.lateness.max().unwrap_or(0), Ordering::Relaxed);
        }

        idx
    }

    /// Read a coherent-per-field snapshot of the published aggregates.
    #[must_use]
    pub fn snapshot(&self) -> ExecutorCycleSnapshot {
        ExecutorCycleSnapshot {
            p50_ns: self.pub_p50.load(Ordering::Relaxed),
            p95_ns: self.pub_p95.load(Ordering::Relaxed),
            p99_ns: self.pub_p99.load(Ordering::Relaxed),
            min_ns: self.pub_min.load(Ordering::Relaxed),
            max_ns: self.pub_max.load(Ordering::Relaxed),
            max_jitter_ns: self.pub_max_jitter.load(Ordering::Relaxed),
            max_lateness_ns: self.pub_max_lateness.load(Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bucket_midpoint;

    // S=8 segments, W=256 exact window.
    type Stats = ExecutorCycleStats<8, 256>;

    #[test]
    fn completed_cycle_folds_samples_and_returns_index() {
        let mut s = Stats::new(1000);
        let idx = s.record_cycle(Some(1000), Some(50), Some(120));
        assert_eq!(idx, 0);
        let snap = s.snapshot();
        assert_eq!(snap.p50_ns, bucket_midpoint(crate::bucket_index(1000)));
        assert_eq!(snap.min_ns, 1000); // exact
        assert_eq!(snap.max_ns, 1000);
        assert_eq!(snap.max_jitter_ns, 50);
        assert_eq!(snap.max_lateness_ns, 120);
    }

    #[test]
    fn fault_is_poison_safe_but_still_advances_index() {
        let mut s = Stats::new(1000);
        let idx0 = s.record_cycle(None, None, None);
        assert_eq!(idx0, 0);
        let after = s.snapshot();
        assert_eq!(after.min_ns, 0);
        assert_eq!(after.p50_ns, 0);
        assert_eq!(after.max_jitter_ns, 0);
        let idx1 = s.record_cycle(Some(2000), Some(10), Some(-5));
        assert_eq!(idx1, 1);
        assert_eq!(s.cycles_recorded(), 2);
        assert_eq!(s.snapshot().min_ns, 2000);
    }

    #[test]
    fn lateness_window_keeps_max_magnitude_of_signed_values() {
        let mut s = Stats::new(1000);
        s.record_cycle(Some(1), Some(0), Some(-300));
        s.record_cycle(Some(1), Some(0), Some(100));
        assert_eq!(s.snapshot().max_lateness_ns, 300);
    }

    #[test]
    fn empty_reports_zero() {
        let s = Stats::new(1000);
        let snap = s.snapshot();
        assert_eq!(snap.p50_ns, 0);
        assert_eq!(snap.p95_ns, 0);
        assert_eq!(snap.p99_ns, 0);
        assert_eq!(snap.min_ns, 0);
        assert_eq!(snap.max_ns, 0);
        assert_eq!(snap.max_jitter_ns, 0);
        assert_eq!(snap.max_lateness_ns, 0);
    }

    #[test]
    fn partial_fault_updates_present_arms_only() {
        let mut s = Stats::new(1000);
        // Seed a good cycle so the duration atomics hold known values.
        s.record_cycle(Some(5000), Some(1), Some(1));
        let before = s.snapshot();
        // Partial fault: took missing, jitter+lateness present.
        let idx = s.record_cycle(None, Some(99), Some(-250));
        assert_eq!(idx, 1);
        let after = s.snapshot();
        // Duration atomics unchanged (took was None).
        assert_eq!(after.p50_ns, before.p50_ns);
        assert_eq!(after.min_ns, before.min_ns);
        assert_eq!(after.max_ns, before.max_ns);
        // Jitter + lateness updated (their Some arms ran).
        assert_eq!(after.max_jitter_ns, 99);
        assert_eq!(after.max_lateness_ns, 250);
    }
}
