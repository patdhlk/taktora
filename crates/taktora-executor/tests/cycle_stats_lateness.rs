//! `REQ_0106` — scan-count-anchored signed deadline lateness, asserted
//! deterministically via an injected [`MockClock`]. Four properties:
//!
//! 1. **Accumulation under steady drift** — a constant sub-period slip every
//!    cycle makes lateness grow linearly and exceed one period.
//! 2. **Coalesced catch-up pair (issue #46)** — a late wake followed by a
//!    short catch-up reports one transient positive spike and heals; the
//!    pre-fix `round(actual_period/period).max(1)` reconstruction instead
//!    fabricated a permanent negative step.
//! 3. **Missed period without a dispatcher skip signal** — honestly reported
//!    as a persistent offset (healing requires the explicit `REQ_0840`
//!    signal; the mock-clock gap here is not a real dispatcher skip).
//! 4. **Per-task grid epoch** — each task anchors at its own first dispatch —
//!    the first sample is exactly zero and later samples stay bounded under
//!    real-time interleave.
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use taktora_executor::{
    ControlFlow, CycleObservation, DispatchMode, Executor, MockClock, Observer, item_with_triggers,
};

const PERIOD_NS: u64 = 10_000_000; // 10 ms nominal — matches the declared interval
const DRIFT_NS: u64 = 2_000_000; // 2 ms steady slip per cycle (< PERIOD/2)

/// Records every pushed lateness sample for exact per-cycle assertions.
#[derive(Default)]
struct Recorder {
    lateness: Mutex<Vec<Option<i64>>>,
}

impl Observer for Recorder {
    fn on_cycle_stats(&self, obs: &CycleObservation) {
        self.lateness.lock().unwrap().push(obs.lateness_ns);
    }
}

/// Routes observations to per-task recorders by `task_index`.
struct Splitter {
    a: Arc<Recorder>,
    b: Arc<Recorder>,
}
impl Observer for Splitter {
    fn on_cycle_stats(&self, obs: &CycleObservation) {
        let r = if obs.task_index == 0 {
            &self.a
        } else {
            &self.b
        };
        r.lateness.lock().unwrap().push(obs.lateness_ns);
    }
}

fn run_with_advances(
    advances: impl Fn(u64) -> u64 + Send + 'static,
    cycles: usize,
) -> (Vec<Option<i64>>, u64) {
    let clock = MockClock::new();
    let body_clock = clock.clone();
    let n = Arc::new(AtomicU64::new(0));
    let recorder = Arc::new(Recorder::default());

    let mut exec = Executor::builder()
        .worker_threads(0)
        .stats_window(256)
        .clock(Arc::new(clock))
        // Legacy dispatch is forced: these tests script the telemetry clock
        // and assert exact per-cycle figures, so they must not depend on
        // real-time dispatcher behavior. Legacy never injects a REQ_0840
        // skip signal (Grid under a starved runner could), and the Grid
        // ferry has its own dedicated test (cycle_stats_skip_signal.rs).
        .dispatch_mode(DispatchMode::Legacy)
        .observer(Arc::clone(&recorder) as Arc<dyn Observer>)
        .build()
        .expect("build executor");

    exec.add(item_with_triggers(
        |d| {
            d.interval(Duration::from_millis(10));
            Ok(())
        },
        move |_ctx| {
            let i = n.fetch_add(1, Ordering::Relaxed);
            body_clock.advance(advances(i));
            Ok(ControlFlow::Continue)
        },
    ))
    .expect("add task");

    exec.run_n(cycles).expect("run_n");
    let max = exec.stats_snapshot().per_task[0].max_lateness_ns;
    let lateness = recorder.lateness.lock().unwrap().clone();
    (lateness, max)
}

#[test]
fn lateness_accumulates_exactly_under_steady_drift() {
    // Cycle n starts n*DRIFT past its grid point: lateness(n) = n*DRIFT.
    let (lateness, max) = run_with_advances(|_| PERIOD_NS + DRIFT_NS, 40);
    assert_eq!(lateness.len(), 40);
    let expected = 39 * DRIFT_NS;
    assert_eq!(max, expected, "max_lateness must be (N-1)*DRIFT exactly");
    for (n, l) in lateness.iter().enumerate() {
        let n = i64::try_from(n).unwrap();
        assert_eq!(*l, Some(n * i64::try_from(DRIFT_NS).unwrap()), "cycle {n}");
    }
}

#[test]
fn coalesced_catch_up_pair_spikes_once_and_heals_without_negatives() {
    // The issue #46 scenario: cycle 10's body advances 1.6 PERIOD (a late
    // wake), cycle 11's advances 0.4 PERIOD (the catch-up back onto the
    // grid). Scan-count anchoring reports exactly one +0.6 PERIOD spike at
    // cycle 11 and zero everywhere else. The pre-fix reconstruction rounded
    // 1.6P to TWO slots and forced the 0.4P catch-up to one more — a
    // permanent fabricated -PERIOD from cycle 12 onward.
    let (lateness, max) = run_with_advances(
        |i| match i {
            10 => 16_000_000, // 1.6 * PERIOD
            11 => 4_000_000,  // 0.4 * PERIOD
            _ => PERIOD_NS,
        },
        20,
    );
    assert_eq!(lateness.len(), 20);
    let spike = 6_000_000_i64; // +0.6 * PERIOD
    assert_eq!(
        max,
        spike.unsigned_abs(),
        "exactly one transient +0.6 PERIOD spike"
    );
    for (n, l) in lateness.iter().enumerate() {
        let expected = if n == 11 { spike } else { 0 };
        assert_eq!(*l, Some(expected), "cycle {n}");
        assert!(
            l.unwrap() >= 0,
            "cycle {n}: no fabricated negative lateness"
        );
    }
}

#[test]
fn missed_period_without_skip_signal_is_an_honest_persistent_offset() {
    // One body advances 2 PERIOD with no dispatcher skip signal (a mock
    // clock gap, not a real starvation). Every cycle from the gap onward
    // honestly reads +PERIOD late on the task's own grid. The pre-fix
    // reconstruction silently absorbed this (round(2P/P) = 2); healing now
    // requires the explicit REQ_0840 signal — see
    // tests/cycle_stats_skip_signal.rs for the signalled path (later task).
    let (lateness, max) =
        run_with_advances(|i| if i == 10 { 2 * PERIOD_NS } else { PERIOD_NS }, 25);
    assert_eq!(lateness.len(), 25);
    assert_eq!(max, PERIOD_NS);
    let late = i64::try_from(PERIOD_NS).unwrap();
    for (n, l) in lateness.iter().enumerate() {
        let expected = if n >= 11 { late } else { 0 };
        assert_eq!(*l, Some(expected), "cycle {n}");
    }
}

#[test]
fn each_task_anchors_lateness_on_its_own_first_dispatch() {
    // Two cyclic tasks share the mock clock; only task A's body advances it
    // (one PERIOD per scan). Task B (20 ms) starts later and at a different
    // phase — with a per-task epoch its every lateness sample is exactly 0.
    // The pre-fix executor-shared epoch reported B's start phase as a
    // permanent positive offset.
    let clock = MockClock::new();
    let body_clock = clock.clone();
    let rec_a = Arc::new(Recorder::default());
    let rec_b = Arc::new(Recorder::default());

    let mut exec = Executor::builder()
        .worker_threads(0)
        .stats_window(256)
        .clock(Arc::new(clock))
        // Legacy dispatch forced — see run_with_advances for the full rationale.
        .dispatch_mode(DispatchMode::Legacy)
        .observer(Arc::new(Splitter {
            a: Arc::clone(&rec_a),
            b: Arc::clone(&rec_b),
        }) as Arc<dyn Observer>)
        .build()
        .expect("build executor");

    exec.add(item_with_triggers(
        |d| {
            d.interval(Duration::from_millis(10));
            Ok(())
        },
        move |_ctx| {
            body_clock.advance(PERIOD_NS);
            Ok(ControlFlow::Continue)
        },
    ))
    .expect("add task A");

    exec.add(item_with_triggers(
        |d| {
            d.interval(Duration::from_millis(20));
            Ok(())
        },
        move |_ctx| Ok(ControlFlow::Continue),
    ))
    .expect("add task B");

    exec.run_n(40).expect("run_n");

    let a = rec_a.lateness.lock().unwrap().clone();
    let b = rec_b.lateness.lock().unwrap().clone();
    assert!(!a.is_empty() && !b.is_empty(), "both tasks recorded cycles");
    for (n, l) in a.iter().enumerate() {
        assert_eq!(*l, Some(0), "task A cycle {n} on its own grid");
    }
    // Task B's FIRST sample is the regression target and is exact by
    // construction: the per-task epoch anchors at B's own first dispatch
    // (elapsed 0, slot 0 => lateness 0) regardless of how the two Legacy
    // relative timers interleave. The pre-fix executor-shared epoch instead
    // reported B's start phase — at least one A-advance, typically +2·10 ms —
    // from the very first sample.
    assert_eq!(b[0], Some(0), "task B first sample anchors its own epoch");
    // Later B samples ride the real-time A/B interleave: each flip moves
    // B's scripted `pre` by one A-advance (±10 ms), and a coalesced B wake
    // can add one more. Lateness must stay bounded (no accumulation) — the
    // grid-slot fold advancing wrongly would compound past this in a few
    // cycles.
    for (n, l) in b.iter().enumerate().skip(1) {
        let v = l.expect("task B records lateness every cycle");
        assert!(
            v.abs() < 40_000_000,
            "task B cycle {n}: lateness must stay bounded, got {v}"
        );
    }
}
