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
/// Bucket `0` covers `[0, 2)` and is reported as `1`. Used for range
/// queries on the bucket boundary.
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

/// Representative value of bucket `i` for percentile estimates: the
/// **geometric** midpoint of the octave `[2^i, 2^(i+1))`, i.e. `2^i · √2`.
///
/// Geometric (not arithmetic) centring is what minimises the *relative*
/// error across a logarithmic bucket. The estimate is within a factor of
/// `√2` of any value the bucket can hold — at most `+41%` / `−29%`, i.e.
/// [`PERCENTILE_MAX_REL_ERR_PCT`](crate::PERCENTILE_MAX_REL_ERR_PCT). The
/// old lower-edge estimate (`2^i`) was instead biased *systematically low*
/// by up to a full octave (a value just under `2^(i+1)` read back as
/// `2^i`, `−50%`), which silently understated every reported percentile.
///
/// Exact extremes (`min`/`max`) are unaffected — they come from
/// [`MinMaxDeque`](crate::MinMaxDeque), not the histogram, and remain the
/// values to trust for any threshold/SLA decision.
#[must_use]
#[allow(clippy::cast_possible_truncation)] // explicit saturating guard below
pub const fn bucket_midpoint(i: usize) -> u64 {
    // 2^i · √2 ≈ (2^i · 92682) >> 16, since 92682 / 65536 = 1.41421…
    // A u128 intermediate avoids overflow for large `i`; the `> u64::MAX`
    // guard makes the `as u64` cast lossless (mid ≤ u64::MAX in that arm).
    let mid = ((1u128 << i) * 92_682) >> 16;
    if mid > u64::MAX as u128 {
        u64::MAX
    } else {
        mid as u64
    }
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

    /// Percentile estimate, in nanoseconds, as the geometric midpoint
    /// ([`bucket_midpoint`]) of the bucket containing the requested rank.
    /// `permille` is the percentile times 10 (e.g. `500` = p50, `950` =
    /// p95, `990` = p99) and must be in `1..=1000`; `0` degenerates to
    /// `bucket_midpoint(0)` regardless of the data. Returns `0` if empty.
    ///
    /// The estimate carries up to
    /// [`PERCENTILE_MAX_REL_ERR_PCT`](crate::PERCENTILE_MAX_REL_ERR_PCT)
    /// relative error (octave bucketing); use exact `min`/`max` for any
    /// threshold decision.
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
                return bucket_midpoint(b);
            }
        }
        bucket_midpoint(B - 1)
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
        assert_eq!(h.percentile(500), bucket_midpoint(10)); // p50
        assert_eq!(h.percentile(990), bucket_midpoint(10)); // p99
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

    // TEST_0190 (verifies REQ_0100): the geometric-midpoint percentile
    // estimate stays within the *documented* relative-error bound
    // (`PERCENTILE_MAX_REL_ERR_PCT`) on a known reference distribution. This
    // is the achievable bound for the octave layout — not the ≤ 1 % goal of
    // REQ_0852/TEST_0868, which needs a sub-octave layout.
    // Sample values are bounded by 100 ms (1e8 ns) and ranks by 10 000, so
    // every cast below is lossless in practice; the casts are inherent to a
    // floating-point relative-error check.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    #[test]
    fn percentile_estimate_within_documented_error_bound() {
        use crate::PERCENTILE_MAX_REL_ERR_PCT;

        // Deterministic LCG — no clock, no `rand`, fully reproducible.
        let mut state: u64 = 0x2545_F491_4F6C_DD1D;
        let mut next_u01 = || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            // Top 53 bits → a double in [0, 1).
            ((state >> 11) as f64) / ((1u64 << 53) as f64)
        };

        let bound = f64::from(PERCENTILE_MAX_REL_ERR_PCT) / 100.0;

        // dist 0: uniform on [100 ns, 100 ms]; dist 1: exponential, mean 1 ms.
        for dist in 0..2 {
            let mut samples: Vec<u64> = Vec::with_capacity(10_000);
            for _ in 0..10_000 {
                let u = next_u01().max(1e-12);
                let v = if dist == 0 {
                    100.0 + u * (100_000_000.0 - 100.0)
                } else {
                    -(u.ln()) * 1_000_000.0
                };
                samples.push((v.max(1.0)) as u64);
            }

            // Single segment, window == sample count: every sample stays in
            // the live window (no ageing), so the estimate is over the full set.
            let mut hist = RollingHistogram::<BUCKETS, 1>::new(10_000);
            for &s in &samples {
                hist.record(s);
            }
            assert_eq!(hist.count(), 10_000);

            let mut sorted = samples.clone();
            sorted.sort_unstable();

            for &permille in &[500u16, 950, 990] {
                // Mirror RollingHistogram::percentile's rank target so the
                // exact reference picks the same rank the histogram does.
                let target = (10_000u64 * u64::from(permille)).div_ceil(1000) as usize;
                let exact = sorted[target - 1] as f64;
                let est = hist.percentile(permille) as f64;
                let rel = (est - exact).abs() / exact;
                assert!(
                    rel <= bound,
                    "dist {dist} p{permille}: est={est} exact={exact} rel={rel:.3} > bound={bound:.3}"
                );
            }
        }
    }
}
