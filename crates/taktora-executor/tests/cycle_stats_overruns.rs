//! `TEST_0192` — `REQ_0102`: `stats_snapshot().per_task[i].overrun_count` surfaces
//! the per-task overrun counter and agrees with `Executor::overrun_count`.
//!
//! Verified semantics (faithfully modelled here):
//! A budget breach (`execute() duration > budget`) does TWO things atomically in
//! one cycle: increments `overrun_count` AND transitions the task to
//! `Faulted{BudgetExceeded}`. Once Faulted, subsequent wakeups are fault-routed
//! (dispatch is skipped) until `clear_task_fault` is called. Consequently,
//! consecutive overruns REQUIRE a `clear_task_fault` between them — this is the
//! intended contract (`REQ_0070` + `REQ_0102`), NOT a test workaround.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use taktora_executor::{ControlFlow, ExecuteResult, Executor, ExecutorError, item_with_triggers};

#[test]
#[allow(clippy::too_many_lines)] // multi-phase test: phase A/B/C must share state
fn snapshot_overrun_count_agrees_with_api_and_increments_only_on_breach() {
    // Shared flag: 0 = fast (~1ms body), 1 = slow (~15ms body, exceeds 5ms budget).
    let slow = Arc::new(AtomicU64::new(0));
    let slow_for_item = Arc::clone(&slow);

    let mut exec = Executor::builder()
        .worker_threads(0) // deterministic single-threaded run_n
        .stats_window(256)
        .build()
        .expect("build executor");

    // Budget = 5 ms.  A ~1 ms body is within budget; a ~15 ms body exceeds it.
    let task_id = exec
        .add(item_with_triggers(
            |d| -> Result<(), ExecutorError> {
                d.interval(Duration::from_millis(5));
                d.budget(Duration::from_millis(5));
                Ok(())
            },
            move |_ctx| -> ExecuteResult {
                if slow_for_item.load(Ordering::Relaxed) == 0 {
                    std::thread::sleep(Duration::from_millis(1));
                } else {
                    std::thread::sleep(Duration::from_millis(15));
                }
                Ok(ControlFlow::Continue)
            },
        ))
        .expect("add task");

    // --- Phase A: within-budget cycles; overrun_count must stay 0 ---
    // run_n(8): 8 cycles × ~1ms body on a 5ms interval — well within budget.
    slow.store(0, Ordering::SeqCst);
    exec.run_n(8).expect("phase A run_n");

    let snap_a = exec.stats_snapshot();
    let api_a = exec.overrun_count(task_id.clone()).expect("known task");
    assert_eq!(
        snap_a.per_task[0].overrun_count, 0,
        "phase A: no breach yet — snapshot overrun_count must be 0, got {}",
        snap_a.per_task[0].overrun_count
    );
    assert_eq!(
        api_a, 0,
        "phase A: Executor::overrun_count must also be 0, got {api_a}"
    );
    assert_eq!(
        snap_a.per_task[0].overrun_count, api_a,
        "phase A: snapshot and API must agree"
    );

    // --- Phase B: breach #1 ---
    // Flip to slow body; run_n(1) to land exactly one breach.
    // The first cycle with a ~15ms body exceeds the 5ms budget → overrun_count
    // becomes 1 AND the task enters Faulted. Subsequent wakeups in the same
    // run_n call would be fault-routed (skipped), so run_n(1) is safe.
    slow.store(1, Ordering::SeqCst);
    exec.run_n(1).expect("phase B breach 1 run_n");

    let snap_b1 = exec.stats_snapshot();
    let api_b1 = exec.overrun_count(task_id.clone()).expect("known task");
    println!(
        "TEST_0192 phase B breach #1: snapshot.overrun_count={}, api.overrun_count={api_b1}",
        snap_b1.per_task[0].overrun_count,
    );
    assert_eq!(
        snap_b1.per_task[0].overrun_count, 1,
        "phase B breach #1: snapshot must show 1 overrun, got {}",
        snap_b1.per_task[0].overrun_count
    );
    assert_eq!(
        api_b1, 1,
        "phase B breach #1: Executor::overrun_count must be 1, got {api_b1}"
    );
    assert_eq!(
        snap_b1.per_task[0].overrun_count, api_b1,
        "phase B breach #1: snapshot and API must agree"
    );

    // Clear the fault so the task can execute again (required by the
    // fault-on-breach semantics: without clear, subsequent wakeups are skipped
    // and the counter would stay at 1 forever).
    exec.clear_task_fault(task_id.clone()).expect("clear fault");

    // --- Phase B: breach #2 ---
    exec.run_n(1).expect("phase B breach 2 run_n");

    let snap_b2 = exec.stats_snapshot();
    let api_b2 = exec.overrun_count(task_id.clone()).expect("known task");
    println!(
        "TEST_0192 phase B breach #2: snapshot.overrun_count={}, api.overrun_count={api_b2}",
        snap_b2.per_task[0].overrun_count,
    );
    assert_eq!(
        snap_b2.per_task[0].overrun_count, 2,
        "phase B breach #2: snapshot must show 2 overruns, got {}",
        snap_b2.per_task[0].overrun_count
    );
    assert_eq!(
        api_b2, 2,
        "phase B breach #2: Executor::overrun_count must be 2, got {api_b2}"
    );
    assert_eq!(
        snap_b2.per_task[0].overrun_count, api_b2,
        "phase B breach #2: snapshot and API must agree"
    );

    // --- Phase C: switch back to fast; counter must NOT increase further ---
    // REQ_0102: overrun_count is lifetime-monotonic and is NOT reset by
    // clear_task_fault. Fast within-budget cycles must not increment it.
    exec.clear_task_fault(task_id.clone())
        .expect("clear fault for phase C");
    slow.store(0, Ordering::SeqCst);
    exec.run_n(8).expect("phase C run_n");

    let snap_c = exec.stats_snapshot();
    let api_c = exec.overrun_count(task_id).expect("known task");
    println!(
        "TEST_0192 phase C (after fast cycles): snapshot.overrun_count={}, api.overrun_count={api_c}",
        snap_c.per_task[0].overrun_count,
    );
    assert_eq!(
        snap_c.per_task[0].overrun_count, 2,
        "phase C: within-budget cycles must NOT increment the counter (expected 2, got {})",
        snap_c.per_task[0].overrun_count
    );
    assert_eq!(
        api_c, 2,
        "phase C: API counter must still be 2, got {api_c}"
    );
    assert_eq!(
        snap_c.per_task[0].overrun_count, api_c,
        "phase C: snapshot and API must agree"
    );
}
