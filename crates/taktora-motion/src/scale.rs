//! Drive boundary (`REQ_0860`, `ADR_0099`): `f64` units -> integer increments,
//! rounded exactly once; wrapping `i32` target write; unwrapping `i32` read.

/// Per-axis linear scaling. `inc_per_unit` is `f64` for v1.
#[derive(Clone, Copy, Debug)]
pub struct AxisScale {
    /// Encoder increments per engineering unit.
    pub inc_per_unit: f64,
    /// Increment value corresponding to user-zero (home offset).
    pub zero_offset: i64,
}

impl AxisScale {
    /// Convert commanded units -> wrapping `i32` target (the single round).
    ///
    /// The final `as i32` wraps modulo 2^32 by design: `CSP` drives follow
    /// position *deltas*, so the absolute target is allowed to roll over.
    #[must_use]
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        // The truncation/wrap from i64 -> i32 is intentional: CSP drives
        // track deltas so the i32 target wraps modulo 2^32 by design.
    )]
    pub fn to_increments(&self, units: f64) -> i32 {
        #[allow(
            clippy::cast_precision_loss,
            // zero_offset is i64; converting to f64 may lose precision for
            // very large offsets, but practically home offsets fit in f64 exactly.
        )]
        let incs = (units * self.inc_per_unit).round() as i64 + self.zero_offset;
        incs as i32 // wraps modulo 2^32 — CSP drives follow deltas
    }

    /// Convert an unwrapped actual increment count -> engineering units.
    #[must_use]
    #[allow(
        clippy::cast_precision_loss,
        // Converting i64 accumulated position to f64 may lose precision for
        // very large counters; acceptable for engineering-unit feedback display.
    )]
    pub fn to_units(&self, increments: i64) -> f64 {
        (increments - self.zero_offset) as f64 / self.inc_per_unit
    }
}

/// Accumulates a wrapping `i32` actual into a continuous `i64` position.
#[derive(Clone, Copy, Debug, Default)]
pub struct Unwrapper {
    last_raw: i32,
    accum: i64,
    seen: bool,
}

impl Unwrapper {
    /// Feed this cycle's raw `i32` actual; return the continuous position.
    ///
    /// Per-cycle motion must be `< 2^31` increments; a larger single step
    /// aliases to the opposite direction (inherent to wrapping deltas).
    pub fn update(&mut self, raw: i32) -> i64 {
        if !self.seen {
            self.seen = true;
            self.last_raw = raw;
            self.accum = i64::from(raw);
            return self.accum;
        }
        let delta = raw.wrapping_sub(self.last_raw); // signed short-step
        self.accum += i64::from(delta);
        self.last_raw = raw;
        self.accum
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounds_once_and_applies_offset() {
        let s = AxisScale {
            inc_per_unit: 1000.0,
            zero_offset: 50,
        };
        // 1.2345 * 1000.0 is *exactly* 1234.5 in f64; `f64::round` rounds
        // half away from zero, so the single round yields 1235 (not 1234).
        assert_eq!(s.to_increments(1.2345), 1235 + 50);
    }

    #[test]
    #[allow(
        clippy::cast_possible_wrap,
        clippy::cast_lossless,
        // Test arithmetic: casting i32::MAX to f64 and i32 values to i64 is
        // intentional here to construct the exact overflow scenario under test.
    )]
    fn target_write_wraps_i32() {
        let s = AxisScale {
            inc_per_unit: 1.0,
            zero_offset: 0,
        };
        // 2^31 + 5 units -> wraps to i32::MIN + 5
        let big = (i32::MAX as f64) + 6.0;
        assert_eq!(s.to_increments(big), i32::MIN + 5);
    }

    #[test]
    #[allow(
        clippy::cast_lossless,
        // Test arithmetic: casting i32 values to i64 is intentional to
        // construct the exact rollover scenario under test.
    )]
    fn unwrap_is_continuous_across_rollover() {
        let mut u = Unwrapper::default();
        assert_eq!(u.update(i32::MAX - 1), (i32::MAX - 1) as i64);
        // step forward by 4: MAX-1 -> MAX -> MIN -> MIN+1 -> MIN+2
        let acc = u.update(i32::MIN + 2);
        assert_eq!(acc, (i32::MAX - 1) as i64 + 4);
    }
}
