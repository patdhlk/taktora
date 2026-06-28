//! `TEST_0914` — `REQ_0925`: the binding's hook write path performs **zero**
//! heap allocations in steady state. The executor invokes these hooks on its
//! `WaitSet` thread, inside the bounded-time control path, so any allocation
//! there would violate the freedom-from-interference contract of `ADR_0111` /
//! `ADR_0114`.
//!
//! The hooks are exercised directly (rather than through a live executor) so the
//! measurement isolates the binding's own write path from executor-internal
//! work — exactly the code `REQ_0925` constrains.
//!
//! **Differential measurement:** `count(big) − count(small)` cancels the
//! one-time cost of the first folded sample (the EWMA seed) and isolates the
//! per-iteration steady-state allocation count, mirroring the executor's own
//! cycle-stats alloc test.
//!
//! **Negative / harness self-check:** a loop that deliberately allocates a
//! `Vec` each iteration proves the `CountingAllocator` still sees allocations on
//! this thread; if it ever stops, the positive assertion is vacuous.

#![allow(missing_docs)]
#![allow(clippy::cast_possible_wrap)]

use core::time::Duration;
use std::time::Instant;

use taktora_bounded_alloc::CountingAllocator;
use taktora_executor::{CycleObservation, ExecutionMonitor, Observer, TaskId};
use taktora_medkit_binding_executor::ExecutorBinding;

#[global_allocator]
static ALLOC: CountingAllocator = CountingAllocator::new();

fn count_allocs<R>(f: impl FnOnce() -> R) -> (usize, R) {
    ALLOC.reset();
    ALLOC.set_tracking(true);
    let r = f();
    ALLOC.set_tracking(false);
    (ALLOC.alloc_count(), r)
}

const ITERS_BIG: usize = 1000;
const ITERS_SMALL: usize = 100;

/// Drive one full cycle of hooks for a single registered task. `task` is cloned
/// outside the measured region into a value held here; passing it by value to
/// each hook only bumps the `Arc<str>` refcount (no heap).
fn beat(binding: &ExecutorBinding, task: &TaskId, at: Instant) {
    binding.on_app_start(task.clone(), 7, None);
    binding.post_execute(task.clone(), at, Duration::from_micros(150), true);
    binding.on_cycle_stats(&CycleObservation {
        cycle_index: 0,
        task_id: task.clone(),
        task_index: 0,
        faulted: false,
        period_ns: 1_000_000,
        pre_ns: 0,
        actual_period_ns: None,
        jitter_ns: None,
        lateness_ns: None,
        skipped_slots: 0,
        took_ns: None,
    });
    binding.on_app_stop(task.clone());
}

fn run(binding: &ExecutorBinding, task: &TaskId, n: usize) {
    let at = Instant::now();
    for _ in 0..n {
        beat(binding, task, at);
    }
}

#[test]
fn hook_write_path_is_zero_allocation() {
    let binding = ExecutorBinding::with_tasks(["ctrl"]);
    let task = TaskId::from("ctrl");

    // ── Positive case (REQ_0925 assertion) ───────────────────────────────────
    // Warm up so the EWMA seed and any first-sample one-shots are absorbed.
    run(&binding, &task, ITERS_SMALL);
    let (a_small, ()) = count_allocs(|| run(&binding, &task, ITERS_SMALL));
    let (a_big, ()) = count_allocs(|| run(&binding, &task, ITERS_BIG));
    let diff = a_big as i64 - a_small as i64;
    let iters = (ITERS_BIG - ITERS_SMALL) as i64;
    // Round up so any fractional alloc per iteration is detected.
    let per_iter = (diff + iters - 1) / iters;
    assert_eq!(
        per_iter, 0,
        "REQ_0925 violated: ~{per_iter} steady-state allocations per hook cycle \
         (a_small={a_small}, a_big={a_big})"
    );

    // ── Negative case (harness self-check) ───────────────────────────────────
    let (neg_small, ()) = count_allocs(|| {
        for _ in 0..ITERS_SMALL {
            core::hint::black_box(vec![1_u8, 2, 3]);
        }
    });
    let (neg_big, ()) = count_allocs(|| {
        for _ in 0..ITERS_BIG {
            core::hint::black_box(vec![1_u8, 2, 3]);
        }
    });
    let neg_per_iter = ((neg_big as i64 - neg_small as i64) + iters - 1) / iters;
    assert!(
        neg_per_iter >= 1,
        "harness regression: CountingAllocator missed deliberate allocations \
         (neg_small={neg_small}, neg_big={neg_big})"
    );
}
