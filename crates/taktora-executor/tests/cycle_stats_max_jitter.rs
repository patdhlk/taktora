//! `TEST_0191` — a period violation produces an EXACT elevated `max_jitter_ns`
//! (`REQ_0101`), asserted deterministically via an injected [`MockClock`].
//!
//! Jitter is `|actual_period − period|`, where `actual_period` is the pre-to-pre
//! spacing of consecutive dispatches measured on the telemetry clock. By
//! driving the telemetry clock from the task body — each body advances the
//! mock clock by a precise number of nanoseconds — the measured period becomes
//! exactly the amount the previous body advanced, with zero dependence on the
//! host scheduler.
//!
//! Every normal cycle advances the clock by exactly one `PERIOD` (so its
//! successor measures `actual_period == PERIOD`, jitter `0`). Every 5th cycle
//! advances by `PERIOD + DELTA`, so its successor measures a `DELTA` overshoot.
//! The windowed maximum is therefore *exactly* `DELTA` — an equality a
//! wall-clock test could never assert. (The earlier real-sleep version had to
//! settle for `>= 2 ms` because a loaded CI runner could inflate the figure to
//! ~69 ms; the mock clock removes the scheduler from the measurement entirely.)
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use taktora_executor::{Executor, ItemFlow, MockClock, item_with_triggers};

const PERIOD_NS: u64 = 1_000_000; // 1 ms nominal — matches the declared interval
const DELTA_NS: u64 = 5_000_000; // 5 ms overshoot injected every 5th cycle

#[test]
fn max_jitter_reflects_exact_period_violation() {
    let clock = MockClock::new();
    let body_clock = clock.clone();
    let cycle_counter = Arc::new(AtomicU64::new(0));
    let cc = Arc::clone(&cycle_counter);

    let mut exec = Executor::builder()
        .worker_threads(0)
        .stats_window(256)
        .clock(Arc::new(clock))
        .build()
        .expect("build executor");

    exec.add(item_with_triggers(
        |d| {
            // The real interval only paces wakeups; all measured time comes
            // from the mock clock. `period_ns` (used for jitter) equals this
            // interval, so the baseline advance must match it for jitter 0.
            d.interval(Duration::from_millis(1));
            Ok(())
        },
        move |_ctx| {
            let n = cc.fetch_add(1, Ordering::Relaxed);
            // Advance the telemetry clock to *simulate* this cycle's spacing.
            // Every 5th cycle (0-indexed 4, 9, …) overshoots by DELTA; the
            // overshoot is observed as jitter on the *following* cycle.
            let advance = if n % 5 == 4 {
                PERIOD_NS + DELTA_NS
            } else {
                PERIOD_NS
            };
            body_clock.advance(advance);
            Ok(ItemFlow::Continue)
        },
    ))
    .expect("add task");

    exec.run_n(60).expect("run_n");

    let snap = exec.stats_snapshot();
    assert_eq!(snap.per_task.len(), 1, "exactly one task registered");

    let max_jitter_ns = snap.per_task[0].max_jitter_ns;
    // Exact: the only non-zero jitter samples come from the DELTA overshoot,
    // and every overshoot is identical, so the windowed maximum is precisely
    // DELTA. No tolerance band — the mock clock makes this deterministic.
    assert_eq!(
        max_jitter_ns, DELTA_NS,
        "max_jitter_ns must equal the injected {DELTA_NS} ns overshoot exactly \
         (got {max_jitter_ns} ns)"
    );
}
