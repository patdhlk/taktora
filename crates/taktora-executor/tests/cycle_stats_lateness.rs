//! `REQ_0106` — grid-anchored signed deadline lateness accumulates under
//! steady drift.
//!
//! Lateness is the signed offset of a cycle's actual start from its nominal
//! grid point `exec_start + n * period` (n = the cycle index). Unlike jitter
//! (a pre-to-pre delta that is blind to a constant per-cycle offset) and
//! unlike the old `elapsed % period` model (phase-within-period, capped below
//! one period and never negative), the grid-anchored model lets a steady
//! slip *accumulate* across cycles.
//!
//! This test drives a 5 ms-interval task whose body sleeps ~7 ms on EVERY
//! cycle. The single-threaded `WaitSet` must drain the body + barrier before
//! it can service the next interval wakeup, so each fire slips ~2 ms+ past its
//! nominal grid point and the slip compounds. After 40 cycles the windowed
//! `max_lateness_ns` must exceed ONE period by a wide margin (>= 10 ms vs the
//! 5 ms period) — a value the old `% period` model could never produce,
//! proving the new grid-anchored accumulation.
use std::time::Duration;
use taktora_executor::{ControlFlow, Executor, item_with_triggers};

#[test]
fn lateness_accumulates_under_steady_drift() {
    let mut exec = Executor::builder()
        .worker_threads(0)
        .stats_window(256)
        .build()
        .expect("build executor");

    exec.add(item_with_triggers(
        |d| {
            d.interval(Duration::from_millis(5));
            Ok(())
        },
        move |_ctx| {
            // ~7 ms body on a 5 ms grid: every cycle slips ~2 ms+ past its
            // nominal deadline, and (because the next interval cannot fire
            // until this body + barrier finish) the slip accumulates.
            std::thread::sleep(Duration::from_millis(7));
            Ok(ControlFlow::Continue)
        },
    ))
    .expect("add task");

    // 40 iterations @ ~7 ms wall each = ~0.28 s — well under any CI timeout.
    exec.run_n(40).expect("run_n");

    let snap = exec.stats_snapshot();
    assert_eq!(snap.per_task.len(), 1, "exactly one task registered");

    let max_lateness_ns = snap.per_task[0].max_lateness_ns;
    println!("REQ_0106: max_lateness_ns = {max_lateness_ns}");

    // The grid-anchored model accumulates the per-cycle ~2 ms slip. By design
    // it must exceed ONE period (5 ms). We assert >= 10 ms (2x the period):
    // the old `elapsed % period` model is mathematically incapable of ever
    // reporting a magnitude >= one period, so clearing 10 ms is conclusive
    // proof of accumulating signed drift. The true accumulated lateness over
    // 40 cycles of ~2 ms slip is ~80 ms, so 10 ms is a generous,
    // non-flaky lower bound.
    assert!(
        max_lateness_ns >= 10_000_000,
        "expected max_lateness_ns >= 10 ms (got {max_lateness_ns} ns); \
         a 7 ms body on a 5 ms grid must accumulate signed drift well past \
         one period — the old `% period` model could never reach this"
    );
}
