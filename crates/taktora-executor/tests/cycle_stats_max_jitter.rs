//! `TEST_0191` — synthetic period violation produces elevated `max_jitter_ns`
//! (`REQ_0101`).
//!
//! A cyclic task with a 10 ms scan period sleeps for 15 ms on every 5th cycle.
//! The body sleep **exceeds** the period, so the cycle cannot complete within
//! its 10 ms grid slot — `actual_period` (measured pre-to-pre, since the
//! `WaitSet` must drain `barrier()` before servicing the next interval wakeup)
//! is forced to stretch to >= 15 ms on every violation cycle, yielding a
//! guaranteed jitter of |15 ms - 10 ms| = 5 ms regardless of platform timer
//! behaviour. An injected delay *shorter* than the period would instead be
//! absorbed by the interval timer's slack and produce ~zero jitter (this is
//! the bug the earlier 5 ms-on-10 ms version had: it passed on macOS by
//! accident but measured ~0 on Linux CI). After 60 cycles the windowed max
//! jitter must be clearly elevated (>= 2 ms). We assert only that lower bound:
//! jitter *magnitude* is scheduler-dependent (a loaded CI runner can starve
//! the dispatch thread for tens of ms, which the telemetry correctly reports
//! as large jitter — ~69 ms seen on macOS CI), so a tight upper bound would
//! test the CI scheduler rather than `REQ_0101`. A loose 1 s guard catches
//! only a garbage/underflow computation result.
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use taktora_executor::{ControlFlow, Executor, item_with_triggers};

#[test]
fn max_jitter_reflects_synthetic_period_violation() {
    let cycle_counter = Arc::new(AtomicU64::new(0));
    let cc = Arc::clone(&cycle_counter);

    let mut exec = Executor::builder()
        .worker_threads(0)
        .stats_window(256)
        .build()
        .expect("build executor");

    exec.add(item_with_triggers(
        |d| {
            d.interval(Duration::from_millis(10));
            Ok(())
        },
        move |_ctx| {
            let n = cc.fetch_add(1, Ordering::Relaxed);
            // Inject a 15 ms body on every 5th cycle (0-indexed: cycles 4, 9, …).
            // 15 ms > the 10 ms period, so the cycle cannot fit its grid slot and
            // the period is forced to stretch — a real, platform-independent
            // period violation. With 60 cycles there are 12 such violations.
            if n % 5 == 4 {
                std::thread::sleep(Duration::from_millis(15));
            }
            Ok(ControlFlow::Continue)
        },
    ))
    .expect("add task");

    // 60 WaitSet iterations @ 10 ms nominal = ~0.6 s; the 12 violation cycles
    // run ~15 ms each — total wall time well under 2 s.
    exec.run_n(60).expect("run_n");

    let snap = exec.stats_snapshot();
    assert_eq!(snap.per_task.len(), 1, "exactly one task registered");

    let max_jitter_ns = snap.per_task[0].max_jitter_ns;
    println!("TEST_0191: max_jitter_ns = {max_jitter_ns}");

    // Lower bound: at least 2 ms. The 15 ms body on a 10 ms period forces a
    // >= 5 ms period stretch on every violation cycle, so the windowed max
    // jitter is guaranteed well above 2 ms on any platform.
    assert!(
        max_jitter_ns >= 2_000_000,
        "expected max_jitter_ns >= 2 ms (got {max_jitter_ns} ns); \
         the 15 ms body on a 10 ms period must register as elevated jitter"
    );

    // Upper bound: a loose 1 s garbage guard only. We deliberately do NOT
    // assert a tight upper bound: on a shared/loaded CI runner the WaitSet
    // thread can be starved for tens of ms between dispatches, and the
    // telemetry *correctly* reports that as large jitter — that is the metric
    // working, not a bug (observed ~69 ms on macOS CI). A tight ceiling would
    // be testing the CI scheduler, not REQ_0101. The behavioural assertion is
    // the lower bound above; this guard merely catches a garbage/underflow
    // value (e.g. a u64 wrap) that no real measurement in a sub-2 s run could
    // produce.
    assert!(
        max_jitter_ns <= 1_000_000_000,
        "max_jitter_ns = {max_jitter_ns} ns exceeds 1 s — impossible for a \
         sub-2 s run; indicates a computation bug (underflow/overflow), not jitter"
    );
}
