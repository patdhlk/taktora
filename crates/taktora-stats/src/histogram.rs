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
}
