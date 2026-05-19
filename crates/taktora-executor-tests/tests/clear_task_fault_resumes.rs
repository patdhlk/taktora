//! TEST_0816 — REQ_0070: clear_task_fault transitions back to Running
//! and resumes dispatch. A second breach re-fires the full cycle.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use taktora_executor::{
    ControlFlow, ExecuteResult, Executor, ExecutorError, FaultReason, Observer, TaskId,
    item_with_triggers,
};

#[derive(Default)]
struct CountingObserver {
    fault_count: AtomicU64,
    clear_count: AtomicU64,
}

impl Observer for CountingObserver {
    fn on_task_fault(&self, _task: TaskId, _reason: FaultReason) {
        self.fault_count.fetch_add(1, Ordering::SeqCst);
    }
    fn on_task_clear(&self, _task: TaskId) {
        self.clear_count.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn clear_task_fault_resumes_dispatch() {
    let observer = Arc::new(CountingObserver::default());
    let mut exec = Executor::builder()
        .worker_threads(1)
        .observer(Arc::clone(&observer) as Arc<dyn Observer>)
        .build()
        .expect("build");

    let calls = Arc::new(AtomicU64::new(0));
    let calls_for_item = Arc::clone(&calls);
    let task_id = exec
        .add(item_with_triggers(
            |d| -> Result<(), ExecutorError> {
                d.interval(Duration::from_millis(5));
                d.budget(Duration::from_millis(1));
                Ok(())
            },
            move |_ctx| -> ExecuteResult {
                calls_for_item.fetch_add(1, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(3));
                Ok(ControlFlow::Continue)
            },
        ))
        .expect("add");

    // First breach.
    exec.run_for(Duration::from_millis(30)).expect("run 1");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(observer.fault_count.load(Ordering::SeqCst), 1);

    // Clear, then run again — should resume dispatching.
    let prev_calls = calls.load(Ordering::SeqCst);
    exec.clear_task_fault(task_id.clone()).expect("clear");
    assert_eq!(observer.clear_count.load(Ordering::SeqCst), 1);

    exec.run_for(Duration::from_millis(30)).expect("run 2");
    assert!(
        calls.load(Ordering::SeqCst) > prev_calls,
        "execute() should run again after clear; before={prev_calls}, after={}",
        calls.load(Ordering::SeqCst)
    );
    // Second breach re-fires on_task_fault.
    assert_eq!(observer.fault_count.load(Ordering::SeqCst), 2);
}
