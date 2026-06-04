//! `REQ_0105` — `min_ns` / `max_ns` retain the EXACT observed execute-duration
//! extremes, NOT bucket-quantised histogram centroids. Asserted deterministically
//! via an injected [`MockClock`].
//!
//! Each cycle's `took` is measured as the telemetry-clock delta across the task
//! body, so a body that advances the mock clock by a precise amount produces a
//! `took` of exactly that amount. We inject one `SPIKE` body among many `BASE`
//! bodies; the windowed `max_ns` must equal `SPIKE` and `min_ns` must equal
//! `BASE` to the nanosecond.
//!
//! Proof-of-exactness: a bucket lower-edge centroid would land materially below
//! `SPIKE` (the internal histogram's buckets span orders of magnitude at this
//! scale). Reporting `max_ns == SPIKE` exactly can only arise from retaining the
//! raw sample. The earlier real-sleep version could only assert `>= 18 ms`
//! because a loaded CI runner stretched the 20 ms sleep to ~87 ms; the mock
//! clock removes the scheduler from the measurement and lets us assert equality.
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use taktora_executor::{ControlFlow, Executor, MockClock, item_with_triggers};

const BASE_NS: u64 = 1_000_000; // 1 ms ordinary body
const SPIKE_NS: u64 = 20_000_000; // 20 ms one-off spike

#[test]
fn min_max_retain_exact_observed_extremes() {
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
            d.interval(Duration::from_millis(1));
            Ok(())
        },
        move |_ctx| {
            let n = cc.fetch_add(1, Ordering::Relaxed);
            // `took` is the clock delta across the body; advancing by a fixed
            // amount makes the recorded took exactly that amount. One SPIKE
            // among BASE bodies gives distinct, exact min/max extremes.
            let advance = if n == 4 { SPIKE_NS } else { BASE_NS };
            body_clock.advance(advance);
            Ok(ControlFlow::Continue)
        },
    ))
    .expect("add task");

    exec.run_n(20).expect("run_n");

    let snap = exec.stats_snapshot();
    assert_eq!(snap.per_task.len(), 1, "exactly one task registered");

    let s = &snap.per_task[0];

    // Exact retention — not a centroid. A bucket-quantised histogram could
    // never report these values to the nanosecond.
    assert_eq!(
        s.max_ns, SPIKE_NS,
        "max_ns must equal the injected {SPIKE_NS} ns spike exactly (got {})",
        s.max_ns
    );
    assert_eq!(
        s.min_ns, BASE_NS,
        "min_ns must equal the {BASE_NS} ns ordinary body exactly (got {})",
        s.min_ns
    );
    assert!(
        s.min_ns < s.max_ns,
        "distinct extremes: min_ns ({}) < max_ns ({})",
        s.min_ns,
        s.max_ns
    );
}
