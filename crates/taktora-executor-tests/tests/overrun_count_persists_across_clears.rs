//! TEST_0819 — REQ_0102: overrun_count is monotonic and is NOT reset by
//! clear_task_fault. Force two breaches separated by a clear and assert
//! the count == 2 (or >= 2 if multiple breaches landed before the clear).

use std::time::Duration;

use taktora_executor::{ControlFlow, ExecuteResult, Executor, ExecutorError, item_with_triggers};

#[test]
fn overrun_count_is_monotonic_across_clears() {
    let mut exec = Executor::builder()
        .worker_threads(1)
        .build()
        .expect("build");

    let task_id = exec
        .add(item_with_triggers(
            |d| -> Result<(), ExecutorError> {
                d.interval(Duration::from_millis(5));
                d.budget(Duration::from_millis(1));
                Ok(())
            },
            |_ctx| -> ExecuteResult {
                std::thread::sleep(Duration::from_millis(3));
                Ok(ControlFlow::Continue)
            },
        ))
        .expect("add");

    // First breach.
    exec.run_for(Duration::from_millis(30)).expect("run 1");
    let count_1 = exec.overrun_count(task_id.clone()).unwrap();
    assert!(count_1 >= 1, "expected >= 1 overrun, got {count_1}");

    exec.clear_task_fault(task_id.clone()).expect("clear");

    // Second breach.
    exec.run_for(Duration::from_millis(30)).expect("run 2");
    let count_2 = exec.overrun_count(task_id).unwrap();
    assert!(
        count_2 > count_1,
        "overrun_count should grow after second breach; before={count_1}, after={count_2}"
    );
}
