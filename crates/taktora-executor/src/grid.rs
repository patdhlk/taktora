//! Absolute-grid cyclic dispatch: the scheduling time source (`CyclicClock`)
//! and the dispatch-mode toggle (`DispatchMode`) for `REQ_0268` / `ADR_0100`.
//! The pure `GridTimer` state machine lands in a subsequent change.
//!
//! This module is deliberately free of iceoryx2 and of the telemetry
//! `MonotonicClock`: scheduling time is a *distinct* role from telemetry
//! measurement, so a test telemetry clock can never alter dispatch timing.

use std::time::Instant;

/// Monotonic nanosecond time source used for **scheduling** cyclic dispatch.
///
/// Distinct from [`crate::MonotonicClock`] (telemetry) by design: the type
/// distinction guarantees a telemetry mock can never be wired as the scheduler.
/// A future fieldbus distributed-clock source is just another implementation.
pub trait CyclicClock: Send + Sync + 'static {
    /// Nanoseconds since this clock's epoch. Monotonic non-decreasing.
    fn now_nanos(&self) -> u64;
}

/// Production scheduling clock over `CLOCK_MONOTONIC` (via `Instant`).
#[derive(Debug)]
pub struct MonotonicCyclicClock {
    epoch: Instant,
}

impl MonotonicCyclicClock {
    /// Construct a clock whose epoch is the current instant.
    #[must_use]
    pub fn new() -> Self {
        Self {
            epoch: Instant::now(),
        }
    }
}

impl Default for MonotonicCyclicClock {
    fn default() -> Self {
        Self::new()
    }
}

impl CyclicClock for MonotonicCyclicClock {
    fn now_nanos(&self) -> u64 {
        u64::try_from(self.epoch.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }
}

/// Cyclic dispatch timing strategy.
///
/// `Grid` is the absolute-grid timer of `REQ_0268`; `Legacy` is the pre-fix
/// `attach_interval` path, retained behind this toggle only until the Pi5 A/B
/// validates `Grid`, then removed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DispatchMode {
    /// Self-computed absolute grid (default).
    #[default]
    Grid,
    /// iceoryx2 `attach_interval` relative timer (drifts — temporary).
    Legacy,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monotonic_cyclic_clock_is_non_decreasing() {
        let c = MonotonicCyclicClock::new();
        let a = c.now_nanos();
        let b = c.now_nanos();
        assert!(
            b >= a,
            "CLOCK_MONOTONIC must not go backwards: {a} then {b}"
        );
    }

    #[test]
    fn dispatch_mode_defaults_to_grid() {
        assert_eq!(DispatchMode::default(), DispatchMode::Grid);
    }
}
