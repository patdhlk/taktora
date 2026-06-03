//! `REQ_0105` — `min_ns` / `max_ns` retain the EXACT observed execute-duration
//! extremes, NOT bucket-quantised histogram centroids.
//!
//! Proof-of-exactness argument:
//! We inject exactly ONE ~20ms body spike among many ~1ms bodies. If the
//! implementation returned a bucket lower-edge centroid it would be
//! materially below 20ms (the bucket width for the HDR histogram used
//! internally spans orders of magnitude). A reported `max_ns >= 18_000_000`
//! can ONLY arise from retaining the raw sample; a centroid at that scale
//! would land many ms below 18ms. The assertion is the proof.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use taktora_executor::{ControlFlow, Executor, item_with_triggers};

#[test]
fn min_max_retain_exact_observed_extremes() {
    let cycle_counter = Arc::new(AtomicU64::new(0));
    let cc = Arc::clone(&cycle_counter);

    let mut exec = Executor::builder()
        .worker_threads(0) // deterministic run_n
        .stats_window(256)
        .build()
        .expect("build executor");

    // No budget — a slow body must NOT fault the task; we just want samples.
    exec.add(item_with_triggers(
        |d| {
            d.interval(Duration::from_millis(3));
            Ok(())
        },
        move |_ctx| {
            let n = cc.fetch_add(1, Ordering::Relaxed);
            // Inject exactly one ~20ms spike on the 5th cycle (0-indexed: n==4).
            // All other cycles sleep ~1ms so there are many small samples to
            // establish a clear min, making the min < max assertion robust.
            if n == 4 {
                std::thread::sleep(Duration::from_millis(20));
            } else {
                std::thread::sleep(Duration::from_millis(1));
            }
            Ok(ControlFlow::Continue)
        },
    ))
    .expect("add task");

    // 20 cycles: the spike lands at cycle 5, giving the window ample samples
    // of both the ~1ms floor and the ~20ms ceiling.
    // Wall time: 19 × ~1ms + 1 × ~20ms = ~39ms nominal — well under any timeout.
    exec.run_n(20).expect("run_n");

    let snap = exec.stats_snapshot();
    assert_eq!(snap.per_task.len(), 1, "exactly one task registered");

    let s = &snap.per_task[0];
    println!(
        "REQ_0105: min_ns={min_ns}, max_ns={max_ns}, p99_ns={p99_ns}",
        min_ns = s.min_ns,
        max_ns = s.max_ns,
        p99_ns = s.p99_ns,
    );

    // --- Max: must reflect the injected ~20ms spike exactly ---
    // Lower bound 18ms: generous to absorb timer resolution / scheduler jitter,
    // but still far above any centroid a bucket-quantised histogram would report
    // at this magnitude. This is the proof of exact retention.
    assert!(
        s.max_ns >= 18_000_000,
        "max_ns must reflect the injected ~20ms spike (>= 18ms); \
         got {max_ns}ns — a value this low can only arise from bucket-quantised \
         centroid truncation, proving exact retention is broken",
        max_ns = s.max_ns,
    );
    // Upper bound 60ms: generous to absorb macOS/CI scheduler overshoot on
    // sleep(20ms). The proof of exactness is entirely in the >=18ms lower
    // bound — a centroid would land well below that, so the upper bound is
    // just a sanity-check for completely runaway measurements.
    assert!(
        s.max_ns <= 60_000_000,
        "max_ns must be at most 60ms (got {max_ns}ns); \
         the injected sleep was 20ms — a value this large indicates \
         an incorrect measurement or extreme scheduler starvation",
        max_ns = s.max_ns,
    );

    // --- Min: must reflect the ~1ms floor ---
    assert!(
        s.min_ns >= 1,
        "min_ns must be positive (got {min_ns}ns)",
        min_ns = s.min_ns,
    );
    assert!(
        s.min_ns <= 5_000_000,
        "min_ns must reflect the ~1ms floor (got {min_ns}ns — expected <= 5ms)",
        min_ns = s.min_ns,
    );

    // --- Ordering ---
    assert!(
        s.min_ns < s.max_ns,
        "min_ns ({min_ns}) must be strictly less than max_ns ({max_ns})",
        min_ns = s.min_ns,
        max_ns = s.max_ns,
    );
}
