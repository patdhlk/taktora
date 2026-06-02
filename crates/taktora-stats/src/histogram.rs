//! Octave-bucket sliding-window percentile histogram.

/// Number of histogram buckets — one per power-of-two octave.
///
/// Covers `2^0 .. 2^63` nanoseconds (≈ 3.32 buckets per decade),
/// satisfying the "≥ 3 buckets per decade" requirement of
/// `REQ_0100` / `ADR_0060`.
pub const BUCKETS: usize = 64;

/// Map a nanosecond value to its octave bucket index.
///
/// The index is `value.ilog2()` clamped to `0 ..= BUCKETS - 1`; `0` and `1`
/// both map to bucket `0`. Pure, allocation-free, O(1).
#[must_use]
#[allow(clippy::cast_possible_truncation)] // ilog2 ∈ 0..=63, always fits usize
pub fn bucket_index(value_ns: u64) -> usize {
    let octave = value_ns.max(1).ilog2() as usize;
    octave.min(BUCKETS - 1)
}

/// Lower edge (inclusive) of bucket `i`, in nanoseconds: `2^i`.
///
/// Bucket `0` covers `[0, 2)` and is reported as `1`. Used as the
/// bucket-quantised percentile estimate.
///
/// # Panics
///
/// Panics in debug builds if `i >= 64` (the `1u64 << i` shift overflows).
/// Callers pass a [`bucket_index`] result, which is clamped to
/// `0 ..= BUCKETS - 1`, so the bound always holds in practice.
#[must_use]
pub const fn bucket_lower(i: usize) -> u64 {
    1u64 << i
}

/// Sliding-window percentile histogram over octave buckets.
///
/// Implemented as a ring of `S` per-segment bucket-count arrays. Each
/// segment holds up to `window / S` samples; when the current segment
/// fills, the ring advances and the segment it advances into is cleared —
/// ageing out the oldest `window / S` samples in one step (the
/// snapshot-ring of `ADR_0060`). In steady state (after at least `window`
/// samples have been recorded) the live window holds between
/// `(S - 1) * (window / S)` and `S * (window / S)` samples — the upper
/// bound equals `window` exactly when `window` is divisible by `S` (floor
/// division). During initial fill, `count()` grows monotonically from `1`
/// to the steady-state upper bound.
///
/// Single-writer: `record` takes `&mut self`. `percentile` is `&self` and
/// O(`BUCKETS * S`). Allocation-free; all storage is inline arrays.
pub struct RollingHistogram<const B: usize, const S: usize> {
    seg: [[u32; B]; S],
    cur: usize,
    n_in_cur: u32,
    seg_capacity: u32,
}

impl<const B: usize, const S: usize> RollingHistogram<B, S> {
    /// Create a histogram whose live window is approximately `window`
    /// samples, divided into `S` segments. `window` is clamped so each
    /// segment holds at least one sample.
    ///
    /// # Panics
    ///
    /// Fails to compile (const-eval assertion) if `B == 0` or `S == 0`; a
    /// zero bucket count or zero segment count has no meaning and would
    /// divide by zero or index out of bounds in the ring math.
    #[must_use]
    pub fn new(window: u32) -> Self {
        const { assert!(B > 0 && S > 0, "RollingHistogram requires B > 0 and S > 0") }
        // S is a small const-generic segment count; casting to u32 is safe
        // because S > u32::MAX is not a realistic use-case.
        #[allow(clippy::cast_possible_truncation)] // S ≤ u32::MAX by construction; const generic
        let s_u32 = S as u32;
        let seg_capacity = (window / s_u32).max(1);
        Self {
            seg: [[0u32; B]; S],
            cur: 0,
            n_in_cur: 0,
            seg_capacity,
        }
    }

    /// Record one sample (nanoseconds). Amortised O(1); O(`B`) at segment
    /// boundaries (once every `window / S` calls, when the segment it
    /// advances into is cleared).
    pub fn record(&mut self, value_ns: u64) {
        if self.n_in_cur >= self.seg_capacity {
            self.cur = (self.cur + 1) % S;
            self.seg[self.cur] = [0u32; B];
            self.n_in_cur = 0;
        }
        self.seg[self.cur][bucket_index(value_ns)] += 1;
        self.n_in_cur += 1;
    }

    /// Total sample count currently in the window.
    #[must_use]
    pub fn count(&self) -> u64 {
        let mut total = 0u64;
        for s in 0..S {
            for b in 0..B {
                total += u64::from(self.seg[s][b]);
            }
        }
        total
    }

    /// Percentile estimate, in nanoseconds, as the lower edge of the bucket
    /// containing the requested rank. `permille` is the percentile times 10
    /// (e.g. `500` = p50, `950` = p95, `990` = p99) and must be in
    /// `1..=1000`; `0` degenerates to `bucket_lower(0)` regardless of the
    /// data. Returns `0` if empty.
    #[must_use]
    pub fn percentile(&self, permille: u16) -> u64 {
        let total = self.count();
        if total == 0 {
            return 0;
        }
        let target = (total * u64::from(permille)).div_ceil(1000);
        let mut cum = 0u64;
        for b in 0..B {
            for s in 0..S {
                cum += u64::from(self.seg[s][b]);
            }
            if cum >= target {
                return bucket_lower(b);
            }
        }
        bucket_lower(B - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_index_maps_values_to_octaves() {
        assert_eq!(bucket_index(0), 0);
        assert_eq!(bucket_index(1), 0);
        assert_eq!(bucket_index(2), 1);
        assert_eq!(bucket_index(3), 1);
        assert_eq!(bucket_index(4), 2);
        assert_eq!(bucket_index(1023), 9);
        assert_eq!(bucket_index(1024), 10);
        assert_eq!(bucket_index(u64::MAX), BUCKETS - 1);
    }

    #[test]
    fn bucket_lower_is_the_octave_edge() {
        assert_eq!(bucket_lower(0), 1);
        assert_eq!(bucket_lower(1), 2);
        assert_eq!(bucket_lower(10), 1024);
    }

    #[test]
    fn at_least_three_buckets_per_decade() {
        // A decade is ~3.32 octaves, so two values a decade apart must land
        // at least 3 buckets apart (REQ_0100 / ADR_0060).
        assert!(bucket_index(10_000) - bucket_index(1_000) >= 3);
        assert!(bucket_index(1_000_000) - bucket_index(100_000) >= 3);
    }

    #[test]
    fn percentile_of_uniform_fill_is_bucket_quantised() {
        // Window 1000, 4 segments. Record 1000 samples all == 1024 ns
        // (bucket 10). Every percentile is that bucket's lower edge.
        let mut h = RollingHistogram::<BUCKETS, 4>::new(1000);
        for _ in 0..1000 {
            h.record(1024);
        }
        assert_eq!(h.count(), 1000);
        assert_eq!(h.percentile(500), bucket_lower(10)); // p50
        assert_eq!(h.percentile(990), bucket_lower(10)); // p99
    }

    #[test]
    fn percentile_separates_low_and_high_populations() {
        // 900 fast samples (~1us, bucket 9-10) + 100 slow (~1ms, bucket 19-20).
        let mut h = RollingHistogram::<BUCKETS, 4>::new(1000);
        for _ in 0..900 {
            h.record(1_000);
        }
        for _ in 0..100 {
            h.record(1_000_000);
        }
        // p50 lands in the fast population, p99 in the slow population.
        assert!(h.percentile(500) < bucket_lower(15));
        assert!(h.percentile(990) >= bucket_lower(15));
    }

    #[test]
    fn old_samples_age_out_of_the_window() {
        // Window 1000 / 4 segments => 250 samples per segment. Fill the
        // window with slow samples, then push enough fast samples to evict
        // every slow one. The slow population must disappear from the tail.
        let mut h = RollingHistogram::<BUCKETS, 4>::new(1000);
        for _ in 0..1000 {
            h.record(1_000_000); // slow, bucket ~19
        }
        for _ in 0..1000 {
            h.record(1_000); // fast, bucket ~9
        }
        // Window now holds only fast samples; even p99 is fast.
        assert!(h.percentile(990) < bucket_lower(15));
        assert!(h.count() <= 1000);
    }

    #[test]
    fn empty_histogram_reports_zero() {
        let h = RollingHistogram::<BUCKETS, 4>::new(1000);
        assert_eq!(h.count(), 0);
        assert_eq!(h.percentile(500), 0);
    }
}
