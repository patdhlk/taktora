//! TEST_0194 — REQ_0104: the per-cycle telemetry update path performs zero
//! heap allocations in steady state.
//!
//! The executor folds per-cycle telemetry (took / jitter / lateness) on the
//! WaitSet thread for every cyclic task, records the result into
//! `ExecutorCycleStats` via pre-allocated atomic accumulators, constructs a
//! stack-allocated `CycleObservation`, and calls `Observer::on_cycle_stats`.
//! With the default `NoopObserver` wired in, this entire fold must produce
//! zero heap allocations per iteration in steady state.
//!
//! **Why a cyclic (`interval`) task?**  A cyclic trigger is what drives the
//! telemetry fold to execute each iteration; without it the fold path is
//! skipped and REQ_0104 would not be exercised at all.
//!
//! **Differential measurement:** `run_n(big) - run_n(small)` cancels the
//! one-time setup cost (WaitSet construction, trigger attachment, lazy init)
//! and isolates the per-iteration steady-state allocation count.  A warm-up
//! `run_n(small)` before measurement absorbs any first-dispatch init that
//! would otherwise contaminate the small window.
//!
//! **Negative / harness-self-check case:** a second executor whose cyclic
//! body deliberately allocates a `Vec` proves the `CountingAllocator` still
//! has visibility into worker-thread allocations.  If this case ever stops
//! detecting allocations the positive assertion becomes vacuous.
//!
//! All cases live in a single `#[test]` function so Cargo's parallel test
//! runner cannot interleave another test's allocations into the measurement
//! window.  The `CountingAllocator` is process-wide; separate tests in the
//! same binary share the same counting state.

#![allow(missing_docs)]
#![allow(clippy::doc_markdown, clippy::cast_possible_wrap)]

use core::time::Duration;
use taktora_bounded_alloc::CountingAllocator;
use taktora_executor::{Executor, ItemFlow, item_with_triggers};

#[global_allocator]
static ALLOC: CountingAllocator = CountingAllocator::new();

fn count_allocs<R>(f: impl FnOnce() -> R) -> (usize, R) {
    ALLOC.reset();
    ALLOC.set_tracking(true);
    let r = f();
    ALLOC.set_tracking(false);
    (ALLOC.alloc_count(), r)
}

const ITERS_BIG: usize = 100;
const ITERS_SMALL: usize = 10;

/// Returns the ceiling of the average steady-state allocations per dispatch
/// iteration for `exec`.  Rounds up so any fractional alloc per iter is
/// detected (a single allocation spread over N iters still reads as 1).
fn per_iter_allocs(exec: &mut Executor) -> i64 {
    // Warm up: absorb any one-shot init that occurs on the first dispatch.
    exec.run_n(ITERS_SMALL).unwrap();
    let (a_small, ()) = count_allocs(|| exec.run_n(ITERS_SMALL).unwrap());
    let (a_big, ()) = count_allocs(|| exec.run_n(ITERS_BIG).unwrap());
    let diff = a_big as i64 - a_small as i64;
    let iters = (ITERS_BIG - ITERS_SMALL) as i64;
    // Round up so any fractional alloc per iter is detected.
    (diff + iters - 1) / iters
}

#[test]
fn cycle_stats_fold_is_zero_allocation() {
    // ── Positive case (REQ_0104 assertion) ────────────────────────────────
    //
    // A single cyclic task with a no-op body.  The only per-iteration runtime
    // work is the telemetry fold: took/jitter/lateness computation,
    // `ExecutorCycleStats::record_cycle` (pre-allocated atomics), construction
    // of a stack `CycleObservation`, and `NoopObserver::on_cycle_stats`.
    // None of these operations should touch the heap.
    {
        let mut exec = Executor::builder().worker_threads(0).build().unwrap();
        exec.add(item_with_triggers(
            |d| {
                d.interval(Duration::from_millis(1));
                Ok(())
            },
            |_| Ok(ItemFlow::Continue),
        ))
        .unwrap();

        let per_iter = per_iter_allocs(&mut exec);
        assert_eq!(
            per_iter, 0,
            "REQ_0104 violated: ~{per_iter} steady-state allocations per iteration \
             in the cycle-stats fold path (default NoopObserver)"
        );
    }

    // ── Negative case (harness self-check) ────────────────────────────────
    //
    // A cyclic task whose body allocates a Vec every iteration.  The harness
    // MUST catch at least one allocation per iteration; if it doesn't, the
    // CountingAllocator has lost visibility into this thread's allocations and
    // the positive assertion above is meaningless.
    {
        let mut exec = Executor::builder().worker_threads(0).build().unwrap();
        exec.add(item_with_triggers(
            |d| {
                d.interval(Duration::from_millis(1));
                Ok(())
            },
            |_| {
                let v: Vec<u8> = vec![1, 2, 3];
                core::hint::black_box(&v);
                Ok(ItemFlow::Continue)
            },
        ))
        .unwrap();

        // One warm-up run before measuring (mirrors the positive case).
        exec.run_n(ITERS_SMALL).unwrap();
        let (a_small, ()) = count_allocs(|| exec.run_n(ITERS_SMALL).unwrap());
        let (a_big, ()) = count_allocs(|| exec.run_n(ITERS_BIG).unwrap());
        let diff = a_big as i64 - a_small as i64;
        let iters = (ITERS_BIG - ITERS_SMALL) as i64;
        let per_iter_neg = (diff + iters - 1) / iters;

        assert!(
            per_iter_neg >= 1,
            "harness regression: CountingAllocator did not catch deliberate vec! allocations \
             in the cycle-stats negative case (a_small={a_small}, a_big={a_big}, \
             per_iter={per_iter_neg})"
        );
    }
}
