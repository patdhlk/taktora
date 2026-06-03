//! `TEST_0191` — synthetic period violation produces elevated `max_jitter_ns`
//! (`REQ_0101`).
//!
//! A cyclic task with a 10 ms scan period sleeps an extra 5 ms on every 5th
//! cycle.  Because `actual_period` is measured pre-to-pre (the `WaitSet` must
//! drain `barrier()` before it can service the next interval wakeup), the body
//! sleep pushes the subsequent dispatch timestamp forward by roughly the extra
//! sleep duration.  After 60 cycles the windowed max jitter must be at least
//! 1 ms (clearly elevated above zero-jitter baseline) and at most 8 ms
//! (generous upper bound to absorb timer resolution / CI scheduler noise).
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
            // Inject a 5 ms body overrun on every 5th cycle (0-indexed: cycles 4, 9, 14, …).
            // With 60 cycles there are 12 such violations, giving the windowed
            // max jitter plenty of samples to register the elevated delay.
            if n % 5 == 4 {
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok(ControlFlow::Continue)
        },
    ))
    .expect("add task");

    // 60 WaitSet iterations @ 10 ms nominal = ~0.6 s; the 12 extra 5 ms
    // sleeps add ~60 ms on top — total wall time well under 2 s.
    exec.run_n(60).expect("run_n");

    let snap = exec.stats_snapshot();
    assert_eq!(snap.per_task.len(), 1, "exactly one task registered");

    let max_jitter_ns = snap.per_task[0].max_jitter_ns;
    println!("TEST_0191: max_jitter_ns = {max_jitter_ns}");

    // Lower bound: at least 1 ms — the 5 ms body sleep must register as
    // clearly elevated jitter on the following cycle's pre-to-pre delta.
    assert!(
        max_jitter_ns >= 1_000_000,
        "expected max_jitter_ns >= 1 ms (got {max_jitter_ns} ns); \
         the 5 ms injected delay should be visible in the windowed jitter"
    );

    // Upper bound: at most 8 ms — no systematic drift should accumulate
    // beyond the injected 5 ms plus a generous timer-resolution allowance.
    assert!(
        max_jitter_ns <= 8_000_000,
        "expected max_jitter_ns <= 8 ms (got {max_jitter_ns} ns); \
         jitter appears systematically higher than the 5 ms injection"
    );
}
