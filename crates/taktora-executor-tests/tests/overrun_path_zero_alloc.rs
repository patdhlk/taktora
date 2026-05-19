//! TEST_0821 — REQ_0060 / REQ_0104: the cycle-overrun post-execute path
//! does not allocate in steady state. CountingAllocator measures across
//! cycles of a task that consistently breaches its budget.
//!
//! Uses the same differential-measurement pattern as the executor's
//! `no_alloc_dispatch.rs` test: `run_n(big) - run_n(small)` cancels
//! one-time lazy-init allocations and isolates per-iteration cost.
//! Single-threaded (`worker_threads(0)`) so the pool's thread-spawn
//! and inter-thread messaging machinery doesn't pollute the count.

#![allow(clippy::doc_markdown)]

use std::time::Duration;

use taktora_bounded_alloc::CountingAllocator;
use taktora_executor::{ControlFlow, ExecuteResult, Executor, ExecutorError, item_with_triggers};

#[global_allocator]
static ALLOC: CountingAllocator = CountingAllocator::new();

const ITERS_SMALL: usize = 10;
const ITERS_BIG: usize = 100;

fn count_allocs<R>(f: impl FnOnce() -> R) -> (usize, R) {
    ALLOC.reset();
    ALLOC.set_tracking(true);
    let r = f();
    ALLOC.set_tracking(false);
    (ALLOC.alloc_count(), r)
}

#[test]
fn overrun_path_zero_alloc_in_steady_state() {
    // Single-threaded executor — the cycle-overrun post-execute hook
    // runs on the dispatch thread itself, so we don't need workers.
    let mut exec = Executor::builder()
        .worker_threads(0)
        .build()
        .expect("build");

    let _ = exec
        .add(item_with_triggers(
            |d| -> Result<(), ExecutorError> {
                d.interval(Duration::from_millis(1));
                d.budget(Duration::from_millis(1));
                Ok(())
            },
            |_ctx| -> ExecuteResult {
                // Sleep just over the budget — every cycle that actually
                // dispatches main() will breach. The first cycle triggers
                // the Running->Faulted transition; subsequent wakeups
                // either halt main dispatch (Faulted state) or, if cleared,
                // re-fire the overrun counter. Either way, the
                // post-execute fault-detection branch must not allocate.
                std::thread::sleep(Duration::from_micros(1500));
                Ok(ControlFlow::Continue)
            },
        ))
        .expect("add");

    // Warm one round to absorb any one-shot lazy init that lands on the
    // first dispatch iteration.
    exec.run_n(ITERS_SMALL).expect("warm");

    let (a_small, ()) = count_allocs(|| exec.run_n(ITERS_SMALL).expect("small"));
    let (a_big, ()) = count_allocs(|| exec.run_n(ITERS_BIG).expect("big"));

    // Differential: per-iteration steady-state cost in the cycle-overrun
    // post-execute path. Zero is the required envelope.
    let diff = i64::try_from(a_big).unwrap() - i64::try_from(a_small).unwrap();
    let iters = i64::try_from(ITERS_BIG - ITERS_SMALL).unwrap();
    // Round up so any fractional alloc per iter is detected.
    let per_iter = (diff + iters - 1) / iters;

    assert_eq!(
        per_iter, 0,
        "overrun post-execute path allocated ~{per_iter} times per iteration \
         (a_small={a_small}, a_big={a_big}) — REQ_0060/REQ_0104 require zero"
    );
}
