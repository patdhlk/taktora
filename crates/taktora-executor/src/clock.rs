//! Monotonic time source seam for cycle telemetry (`REQ_0101`, `REQ_0105`,
//! `REQ_0106`).
//!
//! `std::time::Instant` is **opaque** — it cannot be constructed from a
//! duration — so it is impossible to feed scripted instants to the telemetry
//! math in a test. The timing path that produces `took` / jitter / lateness
//! therefore reads time through this `u64`-nanosecond abstraction instead of
//! calling [`Instant::now`](std::time::Instant::now) directly.
//!
//! * Production wires in [`SystemClock`] (a thin wrapper over a monotonic
//!   [`Instant`](std::time::Instant) epoch), so behaviour is unchanged.
//! * Tests wire in [`MockClock`] via [`ExecutorBuilder::clock`] and advance it
//!   by hand — typically from inside a task body — so jitter, lateness and
//!   min/max can be asserted to the *exact* nanosecond with no real sleeps and
//!   no dependence on the CI scheduler.
//!
//! Only the telemetry path uses this clock. Scheduling (the iceoryx2 `WaitSet`
//! interval triggers), run-mode deadlines, fault `since_ms`, and the
//! [`ExecutionMonitor`](crate::ExecutionMonitor) callbacks continue to use the
//! real [`Instant`](std::time::Instant) clock, so a mock clock can never alter
//! dispatch or fault behaviour.
//!
//! [`ExecutorBuilder::clock`]: crate::ExecutorBuilder::clock

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// A monotonic nanosecond time source for cycle telemetry.
///
/// Implementations return nanoseconds elapsed since an arbitrary fixed epoch.
/// The value must be monotonic non-decreasing within one clock instance; the
/// epoch itself is unspecified (telemetry only ever takes differences).
pub trait MonotonicClock: Send + Sync + 'static {
    /// Nanoseconds since this clock's epoch. Monotonic non-decreasing.
    fn now_nanos(&self) -> u64;
}

/// Production clock: monotonic nanoseconds since the clock was constructed
/// (typically executor `build()` time).
#[derive(Debug)]
pub struct SystemClock {
    epoch: Instant,
}

impl SystemClock {
    /// Construct a clock whose epoch is the current instant.
    #[must_use]
    pub fn new() -> Self {
        Self {
            epoch: Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl MonotonicClock for SystemClock {
    fn now_nanos(&self) -> u64 {
        u64::try_from(self.epoch.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }
}

/// Test clock: a manually-advanced nanosecond counter.
///
/// `MockClock` is a cloneable handle over a shared counter — clone it, hand one
/// clone to [`ExecutorBuilder::clock`](crate::ExecutorBuilder::clock) and keep
/// the other to drive time from the test (or from inside a task body). Both
/// clones observe the same counter.
///
/// Advancing the clock from inside a cyclic task body is the idiomatic pattern:
/// the body "spends" a precise number of nanoseconds, which the telemetry fold
/// then reads back as the cycle's `took` (and, across cycles, as the measured
/// period feeding jitter and lateness).
#[derive(Clone, Debug, Default)]
pub struct MockClock {
    nanos: Arc<AtomicU64>,
}

impl MockClock {
    /// Construct a mock clock reading `0`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Advance the clock by `delta_ns`, returning the new value.
    pub fn advance(&self, delta_ns: u64) -> u64 {
        self.nanos.fetch_add(delta_ns, Ordering::Release) + delta_ns
    }

    /// Set the clock to the absolute value `ns`.
    pub fn set(&self, ns: u64) {
        self.nanos.store(ns, Ordering::Release);
    }

    /// Read the current value.
    #[must_use]
    pub fn now(&self) -> u64 {
        self.nanos.load(Ordering::Acquire)
    }
}

impl MonotonicClock for MockClock {
    fn now_nanos(&self) -> u64 {
        self.nanos.load(Ordering::Acquire)
    }
}
