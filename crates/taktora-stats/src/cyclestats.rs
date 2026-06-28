//! `CycleStatsCore` — the shared per-quantity telemetry bundle (`BB_0054`):
//! a sliding-window percentile histogram plus an exact windowed min/max,
//! over one nanosecond-valued quantity (e.g. a connector's wire-round
//! duration, `REQ_0262`). Single-writer (`&mut`), allocation-free,
//! `no_std`. Reuses [`RollingHistogram`] and [`MinMaxDeque`] (`BB_0053`).

use crate::{BUCKETS, MinMaxDeque, RollingHistogram};

/// Sliding-window stats for one nanosecond quantity: sub-octave-bucket
/// percentiles (`p50`/`p95`/`p99`) plus exact windowed min/max.
///
/// `S` is the histogram segment count (see [`RollingHistogram`]); `W` is
/// the exact min/max window length (see [`MinMaxDeque`]). The histogram
/// window is a runtime argument to [`CycleStatsCore::new`].
pub struct CycleStatsCore<const S: usize, const W: usize> {
    hist: RollingHistogram<BUCKETS, S>,
    minmax: MinMaxDeque<W>,
}

impl<const S: usize, const W: usize> CycleStatsCore<S, W> {
    /// Create an empty core whose histogram window is approximately
    /// `hist_window` samples.
    #[must_use]
    pub fn new(hist_window: u32) -> Self {
        Self {
            hist: RollingHistogram::new(hist_window),
            minmax: MinMaxDeque::new(),
        }
    }

    /// Record one sample (nanoseconds) into both the histogram and the
    /// exact min/max window.
    pub fn record(&mut self, value_ns: u64) {
        self.hist.record(value_ns);
        self.minmax.record(value_ns);
    }

    /// Percentile estimate (bucket lower edge); `permille` ∈ `1..=1000`.
    #[must_use]
    pub fn percentile(&self, permille: u16) -> u64 {
        self.hist.percentile(permille)
    }

    /// p50 (median) estimate, in nanoseconds.
    #[must_use]
    pub fn p50(&self) -> u64 {
        self.hist.percentile(500)
    }

    /// p95 estimate, in nanoseconds.
    #[must_use]
    pub fn p95(&self) -> u64 {
        self.hist.percentile(950)
    }

    /// p99 estimate, in nanoseconds.
    #[must_use]
    pub fn p99(&self) -> u64 {
        self.hist.percentile(990)
    }

    /// Exact windowed minimum, or `None` if no samples recorded.
    #[must_use]
    pub fn min(&self) -> Option<u64> {
        self.minmax.min()
    }

    /// Exact windowed maximum, or `None` if no samples recorded.
    #[must_use]
    pub fn max(&self) -> Option<u64> {
        self.minmax.max()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bucket_index, bucket_midpoint};

    #[test]
    fn records_into_both_histogram_and_exact_minmax() {
        // S=4 segments, W=64 exact window, histogram window 1000.
        let mut c = CycleStatsCore::<4, 64>::new(1000);
        for v in [1000u64, 1000, 1000, 4000, 256] {
            c.record(v);
        }
        // Percentiles are bucket-quantised; the median sample is 1000 ns.
        assert_eq!(c.p50(), bucket_midpoint(bucket_index(1000)));
        // Exact min/max retain the actual extreme samples, not bucket edges.
        assert_eq!(c.min(), Some(256));
        assert_eq!(c.max(), Some(4000));
    }

    #[test]
    fn empty_core_reports_zero_and_none() {
        let c = CycleStatsCore::<4, 64>::new(1000);
        assert_eq!(c.p50(), 0);
        assert_eq!(c.p99(), 0);
        assert_eq!(c.min(), None);
        assert_eq!(c.max(), None);
    }
}
