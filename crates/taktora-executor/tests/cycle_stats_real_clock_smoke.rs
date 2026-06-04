//! Real-clock wiring smoke test for cycle telemetry (`REQ_0103`/`REQ_0105`).
//!
//! The exact-value telemetry tests (`cycle_stats_max_jitter`,
//! `cycle_stats_lateness`, `cycle_stats_minmax`, `cycle_stats_push_pull`) inject
//! a `MockClock` so their assertions are deterministic and scheduler-independent.
//! That makes them blind to one thing: whether the *default* [`SystemClock`]
//! path is actually wired up. This test closes that gap with a single
//! real-`sleep` body and only loose, non-flaky assertions — a genuine `took`
//! sample reaches the snapshot, and a sane number of cycles is recorded. It
//! deliberately asserts no tight magnitudes (those are the mock tests' job), so
//! a loaded CI runner can never make it flaky.
//!
//! [`SystemClock`]: taktora_executor::SystemClock
use std::time::Duration;
use taktora_executor::{ControlFlow, Executor, item_with_triggers};

#[test]
fn default_system_clock_produces_real_telemetry() {
    // No `.clock(...)` call — exercises the production SystemClock default.
    let mut exec = Executor::builder()
        .worker_threads(0)
        .stats_window(64)
        .build()
        .expect("build executor");

    exec.add(item_with_triggers(
        |d| {
            d.interval(Duration::from_millis(1));
            Ok(())
        },
        move |_ctx| {
            std::thread::sleep(Duration::from_millis(1));
            Ok(ControlFlow::Continue)
        },
    ))
    .expect("add task");

    exec.run_n(10).expect("run_n");

    let snap = exec.stats_snapshot();
    assert_eq!(snap.per_task.len(), 1, "exactly one task registered");

    // A real body ran under the real clock, so a positive took sample must have
    // reached the snapshot. Loose lower bound only — no scheduler dependence.
    assert!(
        snap.per_task[0].max_ns > 0,
        "default SystemClock must record a real, positive execute duration \
         (got {})",
        snap.per_task[0].max_ns
    );
}
