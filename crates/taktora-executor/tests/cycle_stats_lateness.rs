//! `REQ_0106` — grid-anchored signed deadline lateness, asserted deterministically
//! via an injected [`MockClock`]. Two properties:
//!
//! 1. **Accumulation under steady drift** — a constant sub-period slip every
//!    cycle makes lateness grow linearly and exceed one period (the property
//!    a phase-within-period model cannot produce).
//! 2. **Self-healing across a missed wakeup** — a single coalesced wakeup that
//!    skips a whole period re-anchors the lateness grid instead of leaving a
//!    permanent per-cycle offset.
//!
//! Both rely on the grid slot being advanced by the *rounded number of nominal
//! periods elapsed* rather than the raw dispatch count: a slip below half a
//! period rounds to one slot (drift accumulates); a gap of two-plus periods
//! advances multiple slots at once (the skip is absorbed, later cycles read
//! back on-grid). The mock clock makes every figure exact.
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use taktora_executor::{ControlFlow, Executor, MockClock, item_with_triggers};

const PERIOD_NS: u64 = 10_000_000; // 10 ms nominal — matches the declared interval
const DRIFT_NS: u64 = 2_000_000; // 2 ms steady slip per cycle (< PERIOD/2 → accumulates)
const CYCLES: usize = 40;

#[test]
fn lateness_accumulates_exactly_under_steady_drift() {
    let clock = MockClock::new();
    let body_clock = clock.clone();

    let mut exec = Executor::builder()
        .worker_threads(0)
        .stats_window(256)
        .clock(Arc::new(clock))
        .build()
        .expect("build executor");

    exec.add(item_with_triggers(
        |d| {
            d.interval(Duration::from_millis(10));
            Ok(())
        },
        move |_ctx| {
            // Every cycle slips a constant DRIFT (< half a period, so it rounds
            // to one grid slot) — the offset from the ideal grid compounds.
            body_clock.advance(PERIOD_NS + DRIFT_NS);
            Ok(ControlFlow::Continue)
        },
    ))
    .expect("add task");

    exec.run_n(CYCLES).expect("run_n");

    let snap = exec.stats_snapshot();
    assert_eq!(snap.per_task.len(), 1, "exactly one task registered");

    // Exact: cycle n advances one grid slot but starts n*DRIFT past its grid
    // point, so lateness(n) = n*DRIFT; the last recorded cycle is the maximum.
    let expected = (CYCLES as u64 - 1) * DRIFT_NS;
    assert_eq!(
        snap.per_task[0].max_lateness_ns, expected,
        "max_lateness_ns must equal (N-1)*DRIFT = {expected} ns exactly (got {})",
        snap.per_task[0].max_lateness_ns
    );
}

#[test]
fn lateness_self_heals_across_a_missed_wakeup() {
    let clock = MockClock::new();
    let body_clock = clock.clone();
    let n = Arc::new(AtomicU64::new(0));
    let nc = Arc::clone(&n);

    let mut exec = Executor::builder()
        .worker_threads(0)
        .stats_window(256)
        .clock(Arc::new(clock))
        .build()
        .expect("build executor");

    exec.add(item_with_triggers(
        |d| {
            d.interval(Duration::from_millis(10));
            Ok(())
        },
        move |_ctx| {
            // Every cycle is perfectly on-grid (advance exactly one period),
            // except cycle 10 which advances TWO periods — a coalesced/missed
            // wakeup (the WaitSet was starved past a whole period). The grid
            // slot advances by 2 there, absorbing the skip; under the old
            // dispatch-count model this would have left a permanent +PERIOD
            // lateness on every subsequent cycle.
            let i = nc.fetch_add(1, Ordering::Relaxed);
            let advance = if i == 10 { 2 * PERIOD_NS } else { PERIOD_NS };
            body_clock.advance(advance);
            Ok(ControlFlow::Continue)
        },
    ))
    .expect("add task");

    exec.run_n(25).expect("run_n");

    let snap = exec.stats_snapshot();
    assert_eq!(snap.per_task.len(), 1, "exactly one task registered");

    // Exact: the skip is fully absorbed by the grid re-anchoring, so every
    // cycle reports lateness 0 and the windowed max is 0. A permanent +PERIOD
    // bias (the pre-fix behaviour) would surface here as PERIOD_NS.
    assert_eq!(
        snap.per_task[0].max_lateness_ns, 0,
        "a missed wakeup must self-heal (max_lateness 0), not leave a permanent \
         {PERIOD_NS} ns offset (got {})",
        snap.per_task[0].max_lateness_ns
    );
}
