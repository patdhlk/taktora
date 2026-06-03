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
    // Upper bound: a loose 5 s garbage guard only. We deliberately do NOT
    // assert a tight ceiling: `max_ns` is a measured wall-clock duration and a
    // loaded/shared CI runner can stretch a sleep(20ms) to tens of ms (~87 ms
    // observed on macOS CI) — the deque is *correctly* retaining that real
    // sample. A tight ceiling would test the CI scheduler, not REQ_0105. The
    // exactness proof is entirely the >=18ms lower bound vs the bucketed
    // centroid; this guard only catches a garbage/overflow value.
    assert!(
        s.max_ns <= 5_000_000_000,
        "max_ns = {max_ns}ns exceeds 5s — impossible for a sub-second run; \
         indicates a computation bug, not a measured duration",
        max_ns = s.max_ns,
    );

    // --- Min: an exact small sample, distinct from the spike ---
    // `min_ns` is positive (a real sample was retained) and strictly below the
    // 20ms spike, proving the deque holds a distinct *minimum* extreme — not
    // the same sample as max. We do NOT assert a tight "~1ms" ceiling on min:
    // its magnitude is scheduler-dependent (every body sleep can be stretched
    // under load), and min < max is guaranteed by construction (one cycle
    // sleeps 20ms more than all others, so its took is always the largest).
    assert!(
        s.min_ns >= 1,
        "min_ns must be positive (got {min_ns}ns)",
        min_ns = s.min_ns,
    );
    assert!(
        s.min_ns < s.max_ns,
        "min_ns ({min_ns}) must be strictly less than max_ns ({max_ns}) — \
         the deque must retain distinct min and max extremes",
        min_ns = s.min_ns,
        max_ns = s.max_ns,
    );
}
