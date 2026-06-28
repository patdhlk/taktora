//! Sub-octave-bucket sliding-window percentile histogram.
//!
//! Each power-of-two octave is subdivided into `M` equal-width mantissa
//! sub-buckets, giving ≥ 115 buckets per decade across 100 ns … 10 s and a
//! geometric-centroid relative error of ≤ 1 % (`REQ_0852`). The hot
//! `bucket_index` path is integer-only and O(1).

/// Sub-buckets per octave (the mantissa subdivision factor `M`).
///
/// Each octave `[2^k, 2^(k+1))` is split into `M` equal-width (linear in
/// the mantissa) sub-buckets. The widest adjacent-edge ratio within an
/// octave is `(M + 1) / M = 1 + 1/M` (at the octave's low edge), so the
/// geometric-centroid relative error is at most `√(1 + 1/M) − 1`. With
/// `M = 64` that is ≈ 0.78 %, comfortably inside the ≤ 1 % bound of
/// `REQ_0852`.
const M: usize = 64;

/// Number of histogram buckets — `M` sub-buckets per power-of-two octave,
/// across octaves `0 ..= 35`.
///
/// The required range is 100 ns (≈ `2^6`) … 10 s (`10^10` ns ≈ `2^33`); the
/// span carries two octaves of margin above `2^33` and the `bucket_index`
/// clamp folds anything larger into the top bucket. At `M` sub-buckets per
/// octave this yields ≥ 115 buckets per decade across the range — the
/// resolution `REQ_0852` needs for a ≤ 1 % centroid error. Per-segment
/// memory is `BUCKETS * 4` bytes.
pub const BUCKETS: usize = 36 * M;

/// Map a nanosecond value to its sub-octave bucket index.
///
/// `octave = value.ilog2()` selects the power-of-two octave and
/// `sub = ((value − 2^octave) · M) >> octave` the mantissa sub-bucket
/// (`0 ..= M − 1`); the index is `(octave · M + sub)` clamped to
/// `0 ..= BUCKETS − 1`. `0` and `1` both map to bucket `0`. The arithmetic
/// is exact integer-only (a `u128` intermediate prevents overflow for large
/// octaves) — no floating point, no loops, no table search. Pure,
/// allocation-free, O(1), and monotonic non-decreasing in `value`.
#[must_use]
#[allow(clippy::cast_possible_truncation)] // octave ∈ 0..=63; sub ∈ 0..M — both fit usize
pub fn bucket_index(value_ns: u64) -> usize {
    let value = value_ns.max(1);
    let octave = value.ilog2() as usize;
    // sub ∈ 0..M; the u128 intermediate avoids overflow when octave is large.
    let sub = (((u128::from(value) - (1u128 << octave)) * M as u128) >> octave) as usize;
    (octave * M + sub).min(BUCKETS - 1)
}

/// Lower edge (inclusive) of bucket `i`, in nanoseconds.
///
/// With `octave = i / M` and `sub = i % M` the edge is
/// `2^octave + (2^octave · sub) / M` — the low edge of the `sub`-th equal
/// slice of octave `octave`. Bucket `0` covers `[0, …)` and is reported as
/// `1`. A `u128` intermediate keeps the multiply exact.
///
/// # Panics
///
/// Panics in debug builds if `i / M >= 64` (the `1u128 << octave` shift
/// overflows). Callers pass a [`bucket_index`] result (or `i + 1` for the
/// top edge), which stays within `0 ..= BUCKETS`, so the bound always holds.
#[must_use]
#[allow(clippy::cast_possible_truncation)] // octave ≤ BUCKETS/M; result ≤ 2^37 fits u64
pub const fn bucket_lower(i: usize) -> u64 {
    let octave = (i / M) as u32;
    let sub = (i % M) as u128;
    let base = 1u128 << octave;
    (base + (base * sub) / M as u128) as u64
}

/// Representative value of bucket `i` for percentile estimates: the
/// **geometric** centroid `√(lower(i) · lower(i+1))` of its `[lower(i),
/// lower(i+1))` interval.
///
/// Geometric (not arithmetic) centring minimises the *relative* error
/// across the bucket. Because each octave is subdivided into `M` sub-buckets
/// (see [`BUCKETS`]), the centroid is within
/// [`PERCENTILE_MAX_REL_ERR_PCT`](crate::PERCENTILE_MAX_REL_ERR_PCT) — ≤ 1 %
/// — of any value the bucket can hold, across 100 ns … 10 s. This is the
/// cold percentile path only, so the exact `u128` square root is fine.
///
/// Exact extremes (`min`/`max`) are unaffected — they come from
/// [`MinMaxDeque`](crate::MinMaxDeque), not the histogram, and remain the
/// values to trust for any threshold/SLA decision.
#[must_use]
#[allow(clippy::cast_possible_truncation)] // centroid ≤ lower(i+1) ≤ 2^37 fits u64
pub const fn bucket_midpoint(i: usize) -> u64 {
    let lo = bucket_lower(i) as u128;
    let hi = bucket_lower(i + 1) as u128;
    let prod = lo * hi;
    // Round the integer square root to the nearest integer: at the low end of
    // the range (~100 ns) truncation alone would nearly double the relative
    // error, so round half-up to keep the centroid within ≤ 1 % of both edges.
    let root = prod.isqrt();
    let rounded = if prod - root * root > root {
        root + 1
    } else {
        root
    };
    rounded as u64
}

/// Sliding-window percentile histogram over sub-octave buckets.
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
    /// (≤ 1 %) relative error across 100 ns … 10 s (sub-octave bucketing);
    /// use exact `min`/`max` for any threshold decision.
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
    fn bucket_index_maps_small_values_and_clamps() {
        // 0 and 1 share bucket 0; powers of two land on octave boundaries.
        assert_eq!(bucket_index(0), 0);
        assert_eq!(bucket_index(1), 0);
        assert_eq!(bucket_index(2), M); // 2^1, low edge of octave 1
        assert_eq!(bucket_index(4), 2 * M); // 2^2, low edge of octave 2
        assert_eq!(bucket_index(1024), 10 * M); // 2^10, low edge of octave 10
        // Within an octave the mantissa selects an interior sub-bucket.
        assert!(bucket_index(3) > bucket_index(2));
        assert!(bucket_index(3) < bucket_index(4));
        // Anything beyond the covered span saturates at the top bucket.
        assert_eq!(bucket_index(u64::MAX), BUCKETS - 1);
    }

    #[test]
    fn bucket_lower_is_the_sub_octave_edge() {
        assert_eq!(bucket_lower(0), 1); // 2^0, reported as 1
        assert_eq!(bucket_lower(M), 2); // 2^1
        assert_eq!(bucket_lower(10 * M), 1024); // 2^10
        // The first sub-bucket above an octave edge sits just above it.
        assert_eq!(bucket_lower(10 * M + 1), 1024 + 1024 / M as u64);
    }

    #[test]
    fn bucket_index_is_monotonic_non_decreasing() {
        // Sub-octave mapping must never go backwards as the value grows.
        let mut prev = bucket_index(1);
        let mut v = 100u64; // 100 ns, low edge of the required range
        while v <= 10_000_000_000 {
            // … up to 10 s
            let cur = bucket_index(v);
            assert!(cur >= prev, "bucket_index({v}) = {cur} < prev {prev}");
            prev = cur;
            v += v / 97 + 1; // geometric-ish sweep, dense enough to catch dips
        }
    }

    #[test]
    fn each_octave_is_subdivided_into_m_sub_buckets() {
        // Within an octave [2^oct, 2^(oct+1)) there are exactly M buckets,
        // and the octave's low edge is the first of them.
        for oct in 7..33u32 {
            let lo = 1u64 << oct;
            let hi = 1u64 << (oct + 1);
            assert_eq!(bucket_index(hi) - bucket_index(lo), M);
            // The mapping spends the whole octave on M distinct buckets.
            let mut seen = bucket_index(lo);
            let mut distinct = 1usize;
            let mut v = lo + 1;
            while v < hi {
                let b = bucket_index(v);
                if b != seen {
                    distinct += 1;
                    seen = b;
                }
                v += (lo / M as u64).max(1);
            }
            assert_eq!(distinct, M, "octave {oct} should expose M sub-buckets");
        }
    }

    #[test]
    fn at_least_115_buckets_per_decade() {
        // REQ_0852: a ≤1% centroid bound needs ~115 buckets per decade
        // across 100 ns … 10 s. Check several decades inside that range.
        for &(lo, hi) in &[
            (1_000u64, 10_000u64),
            (100_000, 1_000_000),
            (10_000_000, 100_000_000),
            (1_000_000_000, 10_000_000_000),
        ] {
            let span = bucket_index(hi) - bucket_index(lo);
            assert!(span >= 115, "decade {lo}..{hi}: only {span} buckets");
        }
    }

    // Edges/midpoints are bounded by ~2e10 ns here, so the f64 casts in the
    // relative-error check are lossless in practice.
    #[allow(clippy::cast_precision_loss)]
    #[test]
    fn lower_and_midpoint_are_consistent_and_within_one_percent() {
        // For every bucket whose lower edge is inside the required range,
        // lower(i) ≤ midpoint(i) < lower(i+1), and the midpoint is within
        // 1% of both edges (the geometric-centroid intra-bucket bound).
        for i in 0..BUCKETS - 1 {
            let lo = bucket_lower(i);
            let hi = bucket_lower(i + 1);
            // Only the required range matters; the sub-100 ns octaves have no
            // mantissa room (2^octave < M) and collapse harmlessly.
            if lo < 100 || hi > 20_000_000_000 {
                continue;
            }
            let mid = bucket_midpoint(i);
            assert!(lo <= mid, "bucket {i}: midpoint {mid} < lower {lo}");
            assert!(mid < hi, "bucket {i}: midpoint {mid} >= upper {hi}");
            let rel_lo = (mid - lo) as f64 / lo as f64;
            let rel_hi = (hi - mid) as f64 / hi as f64;
            assert!(rel_lo <= 0.01, "bucket {i}: {rel_lo:.4} above lower edge");
            assert!(rel_hi <= 0.01, "bucket {i}: {rel_hi:.4} below upper edge");
        }
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
        let b = bucket_index(1024);
        assert_eq!(h.percentile(500), bucket_midpoint(b)); // p50
        assert_eq!(h.percentile(990), bucket_midpoint(b)); // p99
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
        // Threshold is layout-agnostic: a value between the two populations.
        let mid = bucket_lower(bucket_index(100_000));
        assert!(h.percentile(500) < mid);
        assert!(h.percentile(990) >= mid);
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
        assert!(h.percentile(990) < bucket_lower(bucket_index(100_000)));
        assert!(h.count() <= 1000);
    }

    #[test]
    fn empty_histogram_reports_zero() {
        let h = RollingHistogram::<BUCKETS, 4>::new(1000);
        assert_eq!(h.count(), 0);
        assert_eq!(h.percentile(500), 0);
    }

    // TEST_0190 (verifies REQ_0100): the geometric-centroid percentile
    // estimate stays within the *documented* relative-error bound
    // (`PERCENTILE_MAX_REL_ERR_PCT`) on a known reference distribution. Since
    // REQ_0852 tightened the layout to sub-octave buckets, that documented
    // bound is now ≤ 1 %, so this test and TEST_0868 share the same target.
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

    // TEST_0868 (verifies REQ_0852): the sub-octave layout brings the
    // percentile estimate within a *literal 1 %* relative error on the same
    // reference distributions as TEST_0190. This is the acceptance gate for
    // the sub-octave precision requirement; on the old octave layout the
    // estimate carries ≈ 40 % error here, so this test discriminates the two.
    // Sample values are bounded by 100 ms (1e8 ns) and ranks by 10 000, so
    // every cast below is lossless in practice.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    #[test]
    fn sub_octave_percentile_accuracy_within_one_percent() {
        // Deterministic LCG — no clock, no `rand`, fully reproducible.
        let mut state: u64 = 0x2545_F491_4F6C_DD1D;
        let mut next_u01 = || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((state >> 11) as f64) / ((1u64 << 53) as f64)
        };

        let bound = 0.01; // a literal 1 %
        let mut worst = 0.0f64;

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
                worst = worst.max(rel);
                assert!(
                    rel <= bound,
                    "dist {dist} p{permille}: est={est} exact={exact} rel={rel:.4} > bound={bound:.4}"
                );
            }
        }
        // Surfaced with `--nocapture` for the REQ_0852 acceptance record.
        std::eprintln!("TEST_0868 worst-case relative error: {worst:.4}");
    }
}
