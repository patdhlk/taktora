//! TEST_0815 — REQ_0070: per-task budget overrun transitions to Faulted
//! and halts subsequent dispatch. Also verifies `overrun_count >= 1`
//! after one cycle (REQ_0102).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use taktora_executor::{
    ExecuteResult, Executor, ExecutorError, FaultReason, FaultState, ItemFlow, Observer, TaskId,
    item_with_triggers,
};

#[derive(Default)]
struct CaptureObserver {
    fault_count: AtomicU64,
    last_faulted_task: std::sync::Mutex<Option<TaskId>>,
}

impl Observer for CaptureObserver {
    fn on_task_fault(&self, task: TaskId, _reason: FaultReason) {
        self.fault_count.fetch_add(1, Ordering::SeqCst);
        *self.last_faulted_task.lock().unwrap() = Some(task);
    }
}

#[test]
fn budget_breach_faults_task_and_halts_dispatch() {
    let observer = Arc::new(CaptureObserver::default());
    let mut exec = Executor::builder()
        .worker_threads(1)
        .observer(Arc::clone(&observer) as Arc<dyn Observer>)
        .build()
        .expect("build");

    let call_count = Arc::new(AtomicU64::new(0));
    let call_count_for_item = Arc::clone(&call_count);
    let task_id = exec
        .add(item_with_triggers(
            |d| -> Result<(), ExecutorError> {
                d.interval(Duration::from_millis(5));
                d.budget(Duration::from_millis(1));
                Ok(())
            },
            move |_ctx| -> ExecuteResult {
                call_count_for_item.fetch_add(1, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(3));
                Ok(ItemFlow::Continue)
            },
        ))
        .expect("add");

    // Run for ~50ms = ~10 cycles. The first execute() breaches; subsequent
    // wakeups must NOT call execute() again because the task is Faulted.
    exec.run_for(Duration::from_millis(50)).expect("run");

    let calls = call_count.load(Ordering::SeqCst);
    assert_eq!(
        calls, 1,
        "execute() should be called exactly once; subsequent wakeups halted by Faulted state"
    );
    assert_eq!(
        observer.fault_count.load(Ordering::SeqCst),
        1,
        "on_task_fault should fire exactly once per Running->Faulted transition"
    );

    let state = exec.task_fault_state(task_id.clone()).expect("known task");
    assert!(
        matches!(
            state,
            FaultState::Faulted {
                reason: FaultReason::BudgetExceeded { .. },
                ..
            }
        ),
        "expected Faulted{{BudgetExceeded}}, got {state:?}"
    );

    let count = exec.overrun_count(task_id).expect("known task");
    assert!(count >= 1, "overrun_count >= 1 after breach; got {count}");
}
