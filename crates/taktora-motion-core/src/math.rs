//! `f64` helpers that `core` does not provide under `no_std`.
//!
//! `core` lacks `f64::sqrt`, `f64::abs`, and `f64::rem_euclid` (they live in
//! `std`), so we route them through [`libm`]. Keeping every transcendental in
//! one place makes the crate's floating-point surface auditable and keeps the
//! output bit-reproducible across host and target.

/// `true` iff `x` is a strictly positive, non-NaN number.
///
/// Prefer this over `x > 0.0` at call sites that must reject `NaN`: it keeps
/// the NaN-rejecting negation (`!is_positive(x)`) on a `bool` rather than on a
/// partial-order float comparison.
#[inline]
#[must_use]
pub fn is_positive(x: f64) -> bool {
    x > 0.0
}

/// Non-negative square root, via [`libm::sqrt`].
#[inline]
#[must_use]
pub fn sqrt(x: f64) -> f64 {
    libm::sqrt(x)
}

/// Real cube root, via [`libm::cbrt`]. Used by the S-curve "neither limit
/// reached" case, where the jerk time is `cbrt(D / (2·j_max))`.
#[inline]
#[must_use]
pub fn cbrt(x: f64) -> f64 {
    libm::cbrt(x)
}

/// Absolute value, via [`libm::fabs`].
#[inline]
#[must_use]
pub fn abs(x: f64) -> f64 {
    libm::fabs(x)
}

/// Sign of `x` as `-1.0` / `+1.0`; `0.0` maps to `+1.0`.
#[inline]
#[must_use]
pub fn signum(x: f64) -> f64 {
    if x < 0.0 { -1.0 } else { 1.0 }
}

/// Euclidean remainder `x mod m`, always in `[0, m)` for `m > 0`.
///
/// Equivalent to the std `f64::rem_euclid`, implemented over [`libm::fmod`].
/// Returns `x` unchanged if `m` is not strictly positive (no-op wrap).
#[inline]
#[must_use]
pub fn rem_euclid(x: f64, m: f64) -> f64 {
    if !is_positive(m) {
        return x;
    }
    let r = libm::fmod(x, m);
    if r < 0.0 { r + m } else { r }
}

/// Clamp `x` into `[lo, hi]`. Assumes `lo <= hi`.
#[inline]
#[must_use]
pub fn clamp(x: f64, lo: f64, hi: f64) -> f64 {
    if x < lo {
        lo
    } else if x > hi {
        hi
    } else {
        x
    }
}
