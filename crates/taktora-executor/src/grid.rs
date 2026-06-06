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
/// `attach_interval` path. The [`Default`] is **platform-conditional**: `Grid`
/// on Linux (the production absolute-grid `timerfd` path), `Legacy` on non-Linux
/// dev hosts. On non-Linux `Grid` is only a self-computed-`epoll`-timeout
/// fallback — not the real-time target — and its millisecond-rounding jitter
/// makes tight timing tests flaky on loaded CI, so the stable `attach_interval`
/// path is the better default there. The Linux production behaviour is unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DispatchMode {
    /// Self-computed absolute grid; the production default on Linux.
    Grid,
    /// iceoryx2 `attach_interval` relative timer; the default on non-Linux dev
    /// hosts (and the opt-in legacy path on Linux).
    Legacy,
}

impl Default for DispatchMode {
    fn default() -> Self {
        if cfg!(target_os = "linux") {
            Self::Grid
        } else {
            Self::Legacy
        }
    }
}

/// Pure absolute-grid timer. Holds one nominal target per cyclic task; advances
/// each target by exactly one period per dispatch so lateness never compounds.
///
/// No clock and no I/O: callers pass `now` (read from a [`CyclicClock`]) in, so
/// the whole state machine is deterministic and unit-testable.
//
// `redundant_pub_crate` is allowed: `GridTimer` lives in this private module
// and is driven from `dispatch_loop`, so a `pub(crate)` type here reads as
// redundant under clippy. Every field is read — `epoch` by `take_due`
// (skip-realign), `period_ns`/`next` by the dispatch loop.
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
    /// Per-task skipped-slot carry (`REQ_0840`): slots the realign of the
    /// *previous* dispatch passed over unserved, reported on the task's next
    /// dispatch (backward-looking). `0` in steady state; consumed exactly once.
    carry: Vec<u64>,
    /// Per-task "dispatched at least once". A realign on the very first
    /// dispatch sets no carry: the task's lateness grid anchors at that
    /// dispatch, so earlier slots do not exist on its own grid (`REQ_0840`).
    served: Vec<bool>,
}

impl GridTimer {
    /// `epoch` = scheduling `now_nanos()` at dispatch entry; one `period` per
    /// cyclic task. First target for task *k* is `epoch + period_k`.
    pub(crate) fn new(epoch: u64, periods: Vec<u64>) -> Self {
        let len = periods.len();
        let next = periods.iter().map(|p| epoch.saturating_add(*p)).collect();
        Self {
            epoch,
            period_ns: periods,
            next,
            carry: vec![0; len],
            served: vec![false; len],
        }
    }

    /// Time to sleep until the earliest pending grid target (zero if already
    /// due — a zero `epoll` timeout polls and catches up).
    //
    // Used only on the non-Linux self-computed-timeout path: on Linux the master
    // timerfd owns the wake (the wait blocks with `Duration::MAX`), so this is
    // dead there (REQ_0268 / ADR_0100).
    #[cfg_attr(target_os = "linux", allow(dead_code))]
    pub(crate) fn next_timeout(&self, now: u64) -> Duration {
        // No cyclic targets → no grid-driven wakeup. Return `Duration::MAX`
        // exactly (not `u64::MAX` nanos): the WaitSet treats `Duration::MAX`
        // as "block indefinitely on fds" and dispatches a near-MAX `timed_wait`
        // that overflows to `WaitSetRunError::InternalError`. This keeps an
        // event-only executor blocking on its fds identically to Legacy.
        let Some(earliest) = self.next.iter().copied().min() else {
            return Duration::MAX;
        };
        Duration::from_nanos(earliest.saturating_sub(now))
    }

    /// Current nominal target for task `i` (test/inspection helper).
    #[cfg(test)]
    pub(crate) fn next_target(&self, i: usize) -> u64 {
        self.next[i]
    }

    /// Collect cyclic tasks due at `now` into `due` (cleared first), each
    /// paired with its consumed skipped-slot carry (`REQ_0840`) and how late
    /// this dispatch is past the nominal slot it serves (`late_by`,
    /// `REQ_0106`: the record path back-dates the task's lateness-grid epoch
    /// by the FIRST dispatch's `late_by`, anchoring the grid at the nominal
    /// slot — both values are same-clock differences, so they cross the
    /// scheduling/telemetry clock-domain boundary safely). A due task is
    /// dispatched exactly once; its target then advances by one period in the
    /// normal case, or — if the wake was late by ≥1 whole slot — snaps
    /// closed-form to the next *future* grid point (skip-realign, `ADR_0100`),
    /// recording the abandoned slot count as carry for the task's NEXT
    /// dispatch. Never replays a burst of stale cycles, which is wrong for
    /// cyclic control.
    pub(crate) fn take_due(&mut self, now: u64, due: &mut Vec<(usize, u64, u64)>) {
        due.clear();
        for (i, next) in self.next.iter_mut().enumerate() {
            if now >= *next {
                let carry = std::mem::take(&mut self.carry[i]);
                let period = self.period_ns[i];
                if period == 0 {
                    // Unreachable post-registration (rejected per REQ_0268);
                    // continue skips served/carry bookkeeping intentionally.
                    due.push((i, carry, 0));
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
                    let snapped = self
                        .epoch
                        .saturating_add(slots_passed.saturating_add(1).saturating_mul(period));
                    if self.served[i] {
                        // Abandoned slots strictly after the one served by this
                        // dispatch, up to (exclusive) the realigned target (REQ_0840).
                        self.carry[i] = snapped.saturating_sub(stepped) / period;
                    }
                    snapped
                };
                // In both branches the slot this dispatch SERVES is one
                // period before the realigned target, so `late_by` is the
                // same closed form for the normal and the snap path.
                let late_by = now.saturating_sub(next.saturating_sub(period));
                due.push((i, carry, late_by));
                self.served[i] = true;
            }
        }
    }
}

/// The base tick for a PLC-style master timer: the GCD of all declared cyclic
/// periods (ns), so every task's period is an integer number of base ticks and
/// the single timer hits every task's grid point. Returns 0 when there are no
/// cyclic tasks (caller arms no timer). Zero-valued periods are ignored (they
/// are rejected at registration, `REQ_0268`).
// `redundant_pub_crate`: this module is private, so `pub(crate)` looks redundant,
// but the symbol is consumed by `executor::dispatch_loop` (the master timer).
// `dead_code`: only the Linux master-timer path calls `base_period`; on non-Linux
// the `GridTimer` drives dispatch via `next_timeout`, so it is genuinely unused.
#[allow(clippy::redundant_pub_crate)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn base_period(periods: &[u64]) -> u64 {
    periods.iter().copied().filter(|p| *p != 0).fold(0, gcd)
}

// Called only from `base_period`, so it shares the same non-Linux dead-code fate.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
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
    fn dispatch_mode_default_is_grid_on_linux_legacy_elsewhere() {
        // Production default: the absolute-grid timerfd path on Linux; the
        // stable attach_interval fallback on non-Linux dev hosts (REQ_0268).
        #[cfg(target_os = "linux")]
        assert_eq!(DispatchMode::default(), DispatchMode::Grid);
        #[cfg(not(target_os = "linux"))]
        assert_eq!(DispatchMode::default(), DispatchMode::Legacy);
    }

    #[test]
    fn single_period_advances_on_absolute_grid_with_zero_drift() {
        // period 1000ns, epoch 0. Wake late by a varying jitter each cycle and
        // confirm the *nominal target* never absorbs that jitter (no drift).
        let mut t = GridTimer::new(0, vec![1000]);
        let mut due = Vec::new();

        // Cycle 1: woke at 1005 (5ns late). Due once; next target -> 2000, not 2005.
        t.take_due(1005, &mut due);
        assert_eq!(due, vec![(0, 0, 5)]);
        assert_eq!(t.next_target(0), 2000);

        // Cycle 2: woke at 2012 (12ns late). Due once; next target -> 3000.
        t.take_due(2012, &mut due);
        assert_eq!(due, vec![(0, 0, 12)]);
        assert_eq!(t.next_target(0), 3000);

        // Not yet due at 2999.
        t.take_due(2999, &mut due);
        assert_eq!(due, Vec::<(usize, u64, u64)>::new());
        assert_eq!(t.next_target(0), 3000);
    }

    #[test]
    fn stall_skips_whole_slots_and_dispatches_once() {
        // period 1000, epoch 0. We were starved until 3500 (slots 1,2,3 missed).
        let mut t = GridTimer::new(0, vec![1000]);
        let mut due = Vec::new();

        t.take_due(3500, &mut due);
        // Dispatched exactly once — no burst replay of the 3 missed cycles.
        assert_eq!(due, vec![(0, 0, 500)]);
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
        assert_eq!(due, vec![(0, 0, 0)]);
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
    fn empty_grid_next_timeout_is_duration_max() {
        // No cyclic tasks: the timer must yield `Duration::MAX` exactly so the
        // WaitSet blocks on fds (event-only executor) instead of issuing a
        // near-MAX timed wait that overflows to an InternalError.
        let t = GridTimer::new(0, vec![]);
        assert_eq!(t.next_timeout(0), Duration::MAX);
        assert_eq!(t.next_timeout(12_345), Duration::MAX);
    }

    #[test]
    fn base_period_is_gcd_of_declared_periods() {
        assert_eq!(base_period(&[1_000_000]), 1_000_000); // single task → its period
        assert_eq!(base_period(&[2_000_000, 4_000_000]), 2_000_000); // harmonic → smaller
        assert_eq!(base_period(&[2_000_000, 3_000_000]), 1_000_000); // coprime → gcd
        assert_eq!(base_period(&[1_000_000, 1_000_000]), 1_000_000); // duplicates
        assert_eq!(base_period(&[0, 2_000_000]), 2_000_000); // zero entry ignored
        assert_eq!(base_period(&[]), 0); // no cyclic tasks
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
        assert_eq!(due, vec![(0, 0, 0)]);

        // Next earliest: both targets now at 2000.
        assert_eq!(t.next_timeout(1000), Duration::from_nanos(1000));

        // At 2000: both cadences coincide -> both due in one wake.
        t.take_due(2000, &mut due);
        assert_eq!(due, vec![(0, 0, 0), (1, 0, 0)]);
        assert_eq!(t.next_target(0), 3000);
        assert_eq!(t.next_target(1), 4000);
    }

    #[test]
    fn due_entries_carry_zero_skips_in_steady_state() {
        let mut t = GridTimer::new(0, vec![1000]);
        let mut due = Vec::new();
        t.take_due(1002, &mut due);
        assert_eq!(due, vec![(0, 0, 2)]);
        t.take_due(2005, &mut due);
        assert_eq!(due, vec![(0, 0, 5)]);
    }

    #[test]
    fn realign_carries_abandoned_slots_to_the_next_dispatch_exactly_once() {
        // period 1000, epoch 0. Dispatch on-grid once, then starve.
        let mut t = GridTimer::new(0, vec![1000]);
        let mut due = Vec::new();
        t.take_due(1002, &mut due); // serves slot 1; next -> 2000
        assert_eq!(due, vec![(0, 0, 2)]);

        // Starved until 3500: serves the overdue slot-2 target, realigns to 4000.
        // Slot 3 (t=3000) is abandoned: carry = (4000 - 3000) / 1000 = 1.
        // The starved dispatch itself reports 0 — backward-looking semantics:
        // nothing was passed over between slot 1 (previous) and slot 2 (this).
        t.take_due(3500, &mut due);
        assert_eq!(due, vec![(0, 0, 500)]);
        assert_eq!(t.next_target(0), 4000);

        // The NEXT dispatch consumes the carry (slot 2 -> slot 4 skipped slot 3).
        t.take_due(4000, &mut due);
        assert_eq!(due, vec![(0, 1, 0)]);

        // Consumed exactly once: back to 0 afterwards.
        t.take_due(5000, &mut due);
        assert_eq!(due, vec![(0, 0, 0)]);
    }

    #[test]
    fn realign_on_a_tasks_first_dispatch_sets_no_carry() {
        // Starved before the task ever dispatched: slots before the first
        // dispatch do not exist on the task's own lateness grid (REQ_0840).
        let mut t = GridTimer::new(0, vec![1000]);
        let mut due = Vec::new();
        t.take_due(3500, &mut due); // first dispatch, realigns 2000 -> 4000
        assert_eq!(due, vec![(0, 0, 500)]);
        assert_eq!(t.next_target(0), 4000);
        t.take_due(4000, &mut due); // no carry from the first-dispatch realign
        assert_eq!(due, vec![(0, 0, 0)]);
    }

    #[test]
    fn due_entries_report_late_by_vs_the_last_passed_grid_point() {
        // `late_by` anchors the task's lateness-grid epoch (REQ_0106): it is
        // the distance from the dispatch to the most recent grid point at or
        // before `now`. For a whole-slot miss the abandoned slots are NOT
        // part of it — they do not exist on the task's own grid (REQ_0840) —
        // so the anchor lands on the lattice point this dispatch realigns
        // from, keeping later slots' lateness offset-free.
        let mut t = GridTimer::new(0, vec![1000]);
        let mut due = Vec::new();
        // Late wake within the slot: late vs its own target.
        t.take_due(1700, &mut due);
        assert_eq!(due, vec![(0, 0, 700)]);
        // Whole-slot miss (next=2000 overdue, realign to 4000): late is
        // measured vs the last passed lattice point (3000), not the overdue
        // 2000 target.
        t.take_due(3400, &mut due);
        assert_eq!(due, vec![(0, 0, 400)]);
    }

    #[test]
    fn multi_slot_starvation_carries_the_full_abandoned_count() {
        let mut t = GridTimer::new(0, vec![1000]);
        let mut due = Vec::new();
        t.take_due(1000, &mut due); // first dispatch on-grid; next -> 2000
        // Starved until 5500: serves slot-2 target, realign to 6000.
        // Abandoned: slots at 3000, 4000, 5000 -> carry = (6000 - 3000)/1000 = 3.
        t.take_due(5500, &mut due);
        assert_eq!(due, vec![(0, 0, 500)]);
        assert_eq!(t.next_target(0), 6000);
        t.take_due(6000, &mut due);
        assert_eq!(due, vec![(0, 3, 0)]);
    }

    #[test]
    fn back_to_back_realigns_hand_over_carry_without_loss_or_doubling() {
        // Pins the mem::take-then-reassign ordering across two consecutive
        // starvations: the first realign's carry must be consumed by the second
        // starvation's due entry (not stale/doubled), and the second realign's
        // fresh carry must be delivered on the very next on-grid wake.
        // epoch 0, period 1000.
        let mut t = GridTimer::new(0, vec![1000]);
        let mut due = Vec::new();

        // Dispatch on-grid: serves slot 1 (target 1000); next -> 2000.
        t.take_due(1000, &mut due);
        assert_eq!(due, vec![(0, 0, 0)]);

        // First starvation: now=4500 >= next=2000.
        //   stepped = 2000 + 1000 = 3000; 3000 <= 4500 -> realign.
        //   slots_passed = 4500/1000 = 4; snapped = (4+1)*1000 = 5000.
        //   carry = (5000 - 3000) / 1000 = 2.  Abandoned: slots 3000, 4000.
        //   Due entry reports 0 (backward-looking; nothing before slot-2 skipped).
        t.take_due(4500, &mut due);
        assert_eq!(due, vec![(0, 0, 500)]);
        assert_eq!(t.next_target(0), 5000);

        // Second starvation BEFORE on-grid wake: now=7500 >= next=5000.
        //   mem::take delivers the first realign's carry (2) in this due entry.
        //   stepped = 5000 + 1000 = 6000; 6000 <= 7500 -> realign.
        //   slots_passed = 7500/1000 = 7; snapped = (7+1)*1000 = 8000.
        //   carry = (8000 - 6000) / 1000 = 2.  (Second realign's fresh carry.)
        t.take_due(7500, &mut due);
        assert_eq!(due, vec![(0, 2, 500)]);
        assert_eq!(t.next_target(0), 8000);

        // On-grid wake: consumes the second realign's carry, then drains to 0.
        t.take_due(8000, &mut due);
        assert_eq!(due, vec![(0, 2, 0)]);
        t.take_due(9000, &mut due);
        assert_eq!(due, vec![(0, 0, 0)]);
    }
}
