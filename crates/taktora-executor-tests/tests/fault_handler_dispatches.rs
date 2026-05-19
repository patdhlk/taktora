//! TEST_0818 — REQ_0072: after main item breaches budget, a registered
//! fault-handler item dispatches on subsequent wakeups in place of the
//! main item. clear_task_fault restores main-item dispatch.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use taktora_executor::{ControlFlow, ExecuteResult, Executor, ExecutorError, item_with_triggers};

#[test]
fn fault_handler_dispatches_in_place_of_main() {
    let mut exec = Executor::builder()
        .worker_threads(1)
        .build()
        .expect("build");

    let main_calls = Arc::new(AtomicU64::new(0));
    let handler_calls = Arc::new(AtomicU64::new(0));
    let main_calls_for_item = Arc::clone(&main_calls);
    let handler_calls_for_item = Arc::clone(&handler_calls);

    let task_id = exec
        .add_with_fault_handler(
            item_with_triggers(
                |d| -> Result<(), ExecutorError> {
                    d.interval(Duration::from_millis(5));
                    d.budget(Duration::from_millis(1));
                    Ok(())
                },
                move |_ctx| -> ExecuteResult {
                    main_calls_for_item.fetch_add(1, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(3));
                    Ok(ControlFlow::Continue)
                },
            ),
            item_with_triggers(
                |_d| -> Result<(), ExecutorError> { Ok(()) },
                move |_ctx| -> ExecuteResult {
                    handler_calls_for_item.fetch_add(1, Ordering::SeqCst);
                    Ok(ControlFlow::Continue)
                },
            ),
        )
        .expect("add_with_fault_handler");

    // Wider window than the other fault tests: this one needs MULTIPLE
    // post-fault wakeups (main once, then handler ≥ 1 more times). macOS
    // CI runners have visible jitter in iceoryx2 WaitSet + pool worker
    // setup that occasionally consumed all of an earlier 50ms window
    // before the handler had a chance to dispatch even once.
    exec.run_for(Duration::from_millis(300)).expect("run 1");

    // After the first breach the main item should not run again.
    let m1 = main_calls.load(Ordering::SeqCst);
    let h1 = handler_calls.load(Ordering::SeqCst);
    assert_eq!(m1, 1, "main should run exactly once before fault");
    assert!(
        h1 >= 1,
        "handler should run at least once after fault; got {h1}"
    );

    // Clear and continue — main should resume.
    exec.clear_task_fault(task_id).expect("clear");
    exec.run_for(Duration::from_millis(100)).expect("run 2");

    let m2 = main_calls.load(Ordering::SeqCst);
    assert!(
        m2 > m1,
        "main should run again after clear; m1={m1}, m2={m2}"
    );
}
