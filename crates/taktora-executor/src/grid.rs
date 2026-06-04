//! Absolute-grid cyclic dispatch: the scheduling time source (`CyclicClock`),
//! the dispatch-mode toggle (`DispatchMode`), and the pure `GridTimer` state
//! machine for `REQ_0268` / `ADR_0100`.
//!
//! This module is deliberately free of iceoryx2 and of the telemetry
//! `MonotonicClock`: scheduling time is a *distinct* role from telemetry
//! measurement, so a test telemetry clock can never alter dispatch timing.

use std::time::{Duration, Instant};

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

/// Pure absolute-grid timer. Holds one nominal target per cyclic task; advances
/// each target by exactly one period per dispatch so lateness never compounds.
///
/// No clock and no I/O: callers pass `now` (read from a [`CyclicClock`]) in, so
/// the whole state machine is deterministic and unit-testable.
//
// `redundant_pub_crate` is allowed: `GridTimer` is not yet wired into the
// executor (a later task does that), so a `pub(crate)` type in this private
// module reads as redundant until then. No struct-level `dead_code` allow is
// needed: every field is read — `epoch` is now consumed by `take_due`
// (skip-realign), `period_ns`/`next` by the dispatch loop. Method dead-code is
// allowed on the `impl` block below.
#[allow(clippy::redundant_pub_crate)]
#[derive(Debug)]
pub(crate) struct GridTimer {
    /// Scheduling epoch (ns), sampled once at dispatch-loop entry.
    epoch: u64,
    /// Per cyclic task period (ns); index-aligned with `next`. All cadences
    /// share `epoch`, so every period phase-aligns at the epoch (harmonic grid).
    period_ns: Vec<u64>,
    /// Per cyclic task next absolute grid target (ns); `epoch + slot·period`.
    next: Vec<u64>,
}

#[allow(dead_code)] // not yet wired into the executor (later task)
impl GridTimer {
    /// `epoch` = scheduling `now_nanos()` at dispatch entry; one `period` per
    /// cyclic task. First target for task *k* is `epoch + period_k`.
    pub(crate) fn new(epoch: u64, periods: Vec<u64>) -> Self {
        let next = periods.iter().map(|p| epoch.saturating_add(*p)).collect();
        Self {
            epoch,
            period_ns: periods,
            next,
        }
    }

    /// Time to sleep until the earliest pending grid target (zero if already
    /// due — a zero `epoll` timeout polls and catches up).
    pub(crate) fn next_timeout(&self, now: u64) -> Duration {
        let earliest = self.next.iter().copied().min().unwrap_or(u64::MAX);
        Duration::from_nanos(earliest.saturating_sub(now))
    }

    /// Current nominal target for task `i` (test/inspection helper).
    #[cfg(test)]
    pub(crate) fn next_target(&self, i: usize) -> u64 {
        self.next[i]
    }

    /// Collect cyclic tasks due at `now` into `due` (cleared first). A due task
    /// is dispatched exactly once; its target then advances by one period in the
    /// normal case, or — if the wake was late by ≥1 whole slot — snaps closed-form
    /// to the next *future* grid point (skip-realign, `ADR_0100`). Never replays a
    /// burst of stale cycles, which is wrong for cyclic control.
    pub(crate) fn take_due(&mut self, now: u64, due: &mut Vec<usize>) {
        due.clear();
        for (i, next) in self.next.iter_mut().enumerate() {
            if now >= *next {
                due.push(i);
                let period = self.period_ns[i];
                if period == 0 {
                    continue;
                }
                let stepped = next.saturating_add(period);
                *next = if stepped > now {
                    // Normal case: one period ahead is already in the future.
                    stepped
                } else {
                    // Missed >= 1 whole slot: closed-form snap to the next
                    // future grid point. Dispatch once (above); never burst.
                    let slots_passed = now.saturating_sub(self.epoch) / period;
                    self.epoch
                        .saturating_add(slots_passed.saturating_add(1).saturating_mul(period))
                };
            }
        }
    }
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

    #[test]
    fn single_period_advances_on_absolute_grid_with_zero_drift() {
        // period 1000ns, epoch 0. Wake late by a varying jitter each cycle and
        // confirm the *nominal target* never absorbs that jitter (no drift).
        let mut t = GridTimer::new(0, vec![1000]);
        let mut due = Vec::new();

        // Cycle 1: woke at 1005 (5ns late). Due once; next target -> 2000, not 2005.
        t.take_due(1005, &mut due);
        assert_eq!(due, vec![0]);
        assert_eq!(t.next_target(0), 2000);

        // Cycle 2: woke at 2012 (12ns late). Due once; next target -> 3000.
        t.take_due(2012, &mut due);
        assert_eq!(due, vec![0]);
        assert_eq!(t.next_target(0), 3000);

        // Not yet due at 2999.
        t.take_due(2999, &mut due);
        assert_eq!(due, Vec::<usize>::new());
        assert_eq!(t.next_target(0), 3000);
    }

    #[test]
    fn stall_skips_whole_slots_and_dispatches_once() {
        // period 1000, epoch 0. We were starved until 3500 (slots 1,2,3 missed).
        let mut t = GridTimer::new(0, vec![1000]);
        let mut due = Vec::new();

        t.take_due(3500, &mut due);
        // Dispatched exactly once — no burst replay of the 3 missed cycles.
        assert_eq!(due, vec![0]);
        // Re-aligned to the next *future* slot: floor(3500/1000)+1 = 4 -> 4000.
        assert_eq!(t.next_target(0), 4000);
        assert!(
            t.next_target(0) > 3500,
            "target must be strictly in the future"
        );
    }

    #[test]
    fn stall_realign_is_exact_on_a_slot_boundary() {
        let mut t = GridTimer::new(0, vec![1000]);
        let mut due = Vec::new();
        // Exactly on slot 3's boundary.
        t.take_due(3000, &mut due);
        assert_eq!(due, vec![0]);
        assert_eq!(t.next_target(0), 4000);
    }

    #[test]
    // ns form is deliberate: the grid is a nanosecond domain, so timeouts read
    // clearest in the same unit as the period under test.
    #[allow(clippy::duration_suboptimal_units)]
    fn next_timeout_is_distance_to_earliest_target() {
        let t = GridTimer::new(0, vec![1000]);
        assert_eq!(t.next_timeout(0), Duration::from_nanos(1000));
        assert_eq!(t.next_timeout(250), Duration::from_nanos(750));
        // Already past the target -> zero (catch up immediately).
        assert_eq!(t.next_timeout(1500), Duration::from_nanos(0));
    }

    #[test]
    // ns form is deliberate: targets and periods read clearest in the same unit.
    #[allow(clippy::duration_suboptimal_units)]
    fn multi_period_picks_earliest_and_coalesces_coincident_slots() {
        // Two cadences sharing one epoch: 1000ns and 2000ns (harmonic grid).
        let mut t = GridTimer::new(0, vec![1000, 2000]);
        let mut due = Vec::new();

        // Earliest target is task0 at 1000.
        assert_eq!(t.next_timeout(0), Duration::from_nanos(1000));

        // At 1000: only the 1ms task is due.
        t.take_due(1000, &mut due);
        assert_eq!(due, vec![0]);

        // Next earliest: both targets now at 2000.
        assert_eq!(t.next_timeout(1000), Duration::from_nanos(1000));

        // At 2000: both cadences coincide -> both due in one wake.
        t.take_due(2000, &mut due);
        assert_eq!(due, vec![0, 1]);
        assert_eq!(t.next_target(0), 3000);
        assert_eq!(t.next_target(1), 4000);
    }
}
