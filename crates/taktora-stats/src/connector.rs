//! `ConnectorCycleStats` — per-bus cyclic-connector telemetry aggregator
//! (`BB_0054`). Folds each cycle's wire-round and phase-wait durations and
//! quality flags into sliding-window stats, publishes derived scalars to
//! relaxed atomics for a concurrent pull snapshot (`REQ_0265`), and tracks
//! per-device max consecutive-stale runs (`REQ_0264`). Poison-safe: a
//! faulted cycle (`None` duration) contributes no duration sample, but
//! still advances `cycle_index` and the quality counters (`REQ_0267`).

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use crate::{CycleStatsCore, MinMaxDeque};

/// Borrowed-free value snapshot of the per-bus aggregates (`REQ_0265`
/// pull path). All durations are nanoseconds; `N` is the device count.
/// `0` denotes "no sample yet" for the duration fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConnectorCycleSnapshot<const N: usize> {
    /// Wire-round p50 estimate (ns).
    pub wire_round_p50: u64,
    /// Wire-round p95 estimate (ns).
    pub wire_round_p95: u64,
    /// Wire-round p99 estimate (ns).
    pub wire_round_p99: u64,
    /// Exact windowed minimum wire-round duration (ns).
    pub wire_round_min: u64,
    /// Exact windowed maximum wire-round duration (ns).
    pub wire_round_max: u64,
    /// Exact windowed minimum cycle-phase wait / slack (ns).
    pub phase_wait_min: u64,
    /// Running mean cycle-phase wait / slack (ns).
    pub phase_wait_mean: u64,
    /// Lifetime working-counter-mismatch count.
    pub wc_mismatch_count: u64,
    /// Lifetime not-all-devices-fresh cycle count.
    pub not_all_fresh_count: u64,
    /// Per-device maximum consecutive-stale run observed.
    pub per_device_max_stale: [u32; N],
}

/// Per-bus cyclic-connector telemetry, allocated at connector build time.
///
/// `N` = device count, `S` = histogram segment count, `W` = exact min/max
/// window. Single-writer: [`record_cycle`](Self::record_cycle) takes
/// `&mut self`; [`snapshot`](Self::snapshot) reads the published relaxed
/// atomics (per-field tear-free, not coherent across fields — `REQ_0265`).
pub struct ConnectorCycleStats<const N: usize, const S: usize, const W: usize> {
    // Writer-side sliding-window state.
    wire_round: CycleStatsCore<S, W>,
    phase_wait: MinMaxDeque<W>,
    phase_wait_sum: u64,
    phase_wait_count: u64,
    cycle_index: u64,
    // Per-device current consecutive-stale run (working state).
    per_device_stale_run: [u32; N],

    // Published derived scalars (pull path; relaxed atomics).
    pub_wire_p50: AtomicU64,
    pub_wire_p95: AtomicU64,
    pub_wire_p99: AtomicU64,
    pub_wire_min: AtomicU64,
    pub_wire_max: AtomicU64,
    pub_phase_min: AtomicU64,
    pub_phase_mean: AtomicU64,
    pub_wc_mismatch: AtomicU64,
    pub_not_all_fresh: AtomicU64,
    pub_per_device_max_stale: [AtomicU32; N],
}

impl<const N: usize, const S: usize, const W: usize> ConnectorCycleStats<N, S, W> {
    /// Create an empty aggregator; the wire-round histogram window is
    /// approximately `hist_window` samples.
    #[must_use]
    pub fn new(hist_window: u32) -> Self {
        Self {
            wire_round: CycleStatsCore::new(hist_window),
            phase_wait: MinMaxDeque::new(),
            phase_wait_sum: 0,
            phase_wait_count: 0,
            cycle_index: 0,
            per_device_stale_run: [0u32; N],
            pub_wire_p50: AtomicU64::new(0),
            pub_wire_p95: AtomicU64::new(0),
            pub_wire_p99: AtomicU64::new(0),
            pub_wire_min: AtomicU64::new(0),
            pub_wire_max: AtomicU64::new(0),
            pub_phase_min: AtomicU64::new(0),
            pub_phase_mean: AtomicU64::new(0),
            pub_wc_mismatch: AtomicU64::new(0),
            pub_not_all_fresh: AtomicU64::new(0),
            pub_per_device_max_stale: [const { AtomicU32::new(0) }; N],
        }
    }

    /// Number of cycles recorded so far (also the index that the next
    /// cycle will be assigned).
    #[must_use]
    pub const fn cycles_recorded(&self) -> u64 {
        self.cycle_index
    }

    /// Fold one cycle. Durations are `Some(ns)` on a measured cycle and
    /// `None` on a fault (skipped from the aggregates, poison-safe).
    /// `per_device_stale[i] == true` means device `i` dropped out this
    /// cycle. Returns the `cycle_index` assigned to this cycle; the index
    /// advances on every call, fault included (`REQ_0267`).
    pub fn record_cycle(
        &mut self,
        wire_round_ns: Option<u32>,
        phase_wait_ns: Option<u32>,
        all_devices_fresh: bool,
        wc_ok: bool,
        per_device_stale: &[bool; N],
    ) -> u64 {
        let idx = self.cycle_index;
        self.cycle_index += 1;

        if let Some(ns) = wire_round_ns {
            self.wire_round.record(u64::from(ns));
            self.pub_wire_p50
                .store(self.wire_round.p50(), Ordering::Relaxed);
            self.pub_wire_p95
                .store(self.wire_round.p95(), Ordering::Relaxed);
            self.pub_wire_p99
                .store(self.wire_round.p99(), Ordering::Relaxed);
            self.pub_wire_min
                .store(self.wire_round.min().unwrap_or(0), Ordering::Relaxed);
            self.pub_wire_max
                .store(self.wire_round.max().unwrap_or(0), Ordering::Relaxed);
        }

        if let Some(ns) = phase_wait_ns {
            let v = u64::from(ns);
            self.phase_wait.record(v);
            self.phase_wait_sum = self.phase_wait_sum.saturating_add(v);
            self.phase_wait_count += 1;
            self.pub_phase_min
                .store(self.phase_wait.min().unwrap_or(0), Ordering::Relaxed);
            self.pub_phase_mean.store(
                self.phase_wait_sum / self.phase_wait_count,
                Ordering::Relaxed,
            );
        }

        if !wc_ok {
            self.pub_wc_mismatch.fetch_add(1, Ordering::Relaxed);
        }
        if !all_devices_fresh {
            self.pub_not_all_fresh.fetch_add(1, Ordering::Relaxed);
        }

        for ((&stale, run), maxa) in per_device_stale
            .iter()
            .zip(self.per_device_stale_run.iter_mut())
            .zip(self.pub_per_device_max_stale.iter())
        {
            if stale {
                *run = run.saturating_add(1);
                if *run > maxa.load(Ordering::Relaxed) {
                    maxa.store(*run, Ordering::Relaxed);
                }
            } else {
                *run = 0;
            }
        }

        idx
    }

    /// Read a coherent-per-field snapshot of the published aggregates.
    #[must_use]
    pub fn snapshot(&self) -> ConnectorCycleSnapshot<N> {
        let mut per_device_max_stale = [0u32; N];
        for (dst, src) in per_device_max_stale
            .iter_mut()
            .zip(self.pub_per_device_max_stale.iter())
        {
            *dst = src.load(Ordering::Relaxed);
        }
        ConnectorCycleSnapshot {
            wire_round_p50: self.pub_wire_p50.load(Ordering::Relaxed),
            wire_round_p95: self.pub_wire_p95.load(Ordering::Relaxed),
            wire_round_p99: self.pub_wire_p99.load(Ordering::Relaxed),
            wire_round_min: self.pub_wire_min.load(Ordering::Relaxed),
            wire_round_max: self.pub_wire_max.load(Ordering::Relaxed),
            phase_wait_min: self.pub_phase_min.load(Ordering::Relaxed),
            phase_wait_mean: self.pub_phase_mean.load(Ordering::Relaxed),
            wc_mismatch_count: self.pub_wc_mismatch.load(Ordering::Relaxed),
            not_all_fresh_count: self.pub_not_all_fresh.load(Ordering::Relaxed),
            per_device_max_stale,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bucket_midpoint;

    // N=2 devices, S=4 segments, W=64 exact window.
    type Stats = ConnectorCycleStats<2, 4, 64>;

    #[test]
    fn completed_cycle_folds_durations_and_returns_index() {
        let mut s = Stats::new(1000);
        let idx = s.record_cycle(Some(1000), Some(500), true, true, &[false, false]);
        assert_eq!(idx, 0);
        let snap = s.snapshot();
        assert_eq!(snap.wire_round_p50, bucket_midpoint(9)); // 1000 ns -> bucket 9
        assert_eq!(snap.wire_round_min, 1000); // exact
        assert_eq!(snap.phase_wait_min, 500);
        assert_eq!(snap.phase_wait_mean, 500);
        assert_eq!(snap.wc_mismatch_count, 0);
        assert_eq!(snap.not_all_fresh_count, 0);
        assert_eq!(snap.per_device_max_stale, [0, 0]);
    }

    #[test]
    fn fault_is_poison_safe_but_still_advances_index() {
        let mut s = Stats::new(1000);
        // Hard fault first: both durations None (REQ_0267).
        let idx0 = s.record_cycle(None, None, false, false, &[false, false]);
        assert_eq!(idx0, 0);
        let after_fault = s.snapshot();
        // No duration sample recorded -> aggregates untouched (poison-safe).
        assert_eq!(after_fault.wire_round_min, 0);
        assert_eq!(after_fault.wire_round_p50, 0);
        assert_eq!(after_fault.phase_wait_min, 0);
        // Quality counters still moved.
        assert_eq!(after_fault.wc_mismatch_count, 1);
        assert_eq!(after_fault.not_all_fresh_count, 1);

        // Next good cycle gets index 1 (counter advanced through the fault).
        let idx1 = s.record_cycle(Some(2000), Some(800), true, true, &[false, false]);
        assert_eq!(idx1, 1);
        let snap = s.snapshot();
        assert_eq!(snap.wire_round_min, 2000); // only the good sample counts
    }

    #[test]
    fn tracks_per_device_max_consecutive_stale_run() {
        let mut s = Stats::new(1000);
        // Device 0 stale 3 cycles, then fresh, then stale 1.
        s.record_cycle(Some(1), Some(1), false, true, &[true, false]);
        s.record_cycle(Some(1), Some(1), false, true, &[true, false]);
        s.record_cycle(Some(1), Some(1), false, true, &[true, false]);
        s.record_cycle(Some(1), Some(1), true, true, &[false, false]); // run resets
        s.record_cycle(Some(1), Some(1), false, true, &[true, false]);
        let snap = s.snapshot();
        assert_eq!(snap.per_device_max_stale, [3, 0]); // max run was 3
        assert_eq!(snap.not_all_fresh_count, 4); // 4 not-all-fresh cycles
    }

    #[test]
    fn phase_wait_mean_averages_recorded_samples() {
        let mut s = Stats::new(1000);
        s.record_cycle(Some(1), Some(100), true, true, &[false, false]);
        s.record_cycle(Some(1), Some(300), true, true, &[false, false]);
        let snap = s.snapshot();
        assert_eq!(snap.phase_wait_min, 100);
        assert_eq!(snap.phase_wait_mean, 200); // (100 + 300) / 2
    }
}
