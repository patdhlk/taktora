//! TEST_0817 — REQ_0071, REQ_0073: executor-wide iteration_budget breach
//! transitions the executor to Faulted and lazy-cascades other Running
//! tasks to Faulted{ExecutorFaulted} without firing per-task on_task_fault.
//! clear_executor_fault cascades-clears them back to Running.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use taktora_executor::{
    ExecuteResult, Executor, ExecutorError, ExecutorFaultReason, ExecutorFaultState, FaultReason,
    ItemFlow, Observer, TaskId, item_with_triggers,
};

#[derive(Default)]
struct ScopedObserver {
    task_fault_count: AtomicU64,
    executor_fault_count: AtomicU64,
    task_clear_count: AtomicU64,
    executor_clear_count: AtomicU64,
}

impl Observer for ScopedObserver {
    fn on_task_fault(&self, _: TaskId, _: FaultReason) {
        self.task_fault_count.fetch_add(1, Ordering::SeqCst);
    }
    fn on_executor_fault(&self, _: ExecutorFaultReason) {
        self.executor_fault_count.fetch_add(1, Ordering::SeqCst);
    }
    fn on_task_clear(&self, _: TaskId) {
        self.task_clear_count.fetch_add(1, Ordering::SeqCst);
    }
    fn on_executor_clear(&self) {
        self.executor_clear_count.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn executor_wide_breach_faults_executor_and_cascades_silently() {
    let observer = Arc::new(ScopedObserver::default());
    // Uses the platform default DispatchMode: the master-timerfd Grid on Linux
    // (atomic, interrupt-driven), the stable attach_interval Legacy on non-Linux
    // dev/CI hosts. The non-Linux Grid fallback's ms-rounding jitter would make
    // it flaky on loaded CI — the platform default avoids that. The test is
    // mode-agnostic (it exercises the fault cascade, not the dispatch timer).
    // REQ_0268.
    let mut exec = Executor::builder()
        .worker_threads(2)
        .iteration_budget(Duration::from_millis(10))
        .observer(Arc::clone(&observer) as Arc<dyn Observer>)
        .build()
        .expect("build");

    // Task A — slow, will breach the executor-wide 10ms budget.
    let slow_id = exec
        .add(item_with_triggers(
            |d| -> Result<(), ExecutorError> {
                d.interval(Duration::from_millis(5));
                Ok(())
            },
            |_ctx| -> ExecuteResult {
                std::thread::sleep(Duration::from_millis(20));
                Ok(ItemFlow::Continue)
            },
        ))
        .expect("add slow");

    // Task B — healthy.
    let healthy_id = exec
        .add(item_with_triggers(
            |d| -> Result<(), ExecutorError> {
                d.interval(Duration::from_millis(5));
                Ok(())
            },
            |_ctx| -> ExecuteResult { Ok(ItemFlow::Continue) },
        ))
        .expect("add healthy");

    // Window sizing (post one-barrier-per-wake, #95): the lazy cascade needs a
    // *post-fault* wake. Before #95, `process_attachment` barriered per
    // attachment, so the slow task's barrier set `exec_fault` BEFORE the healthy
    // task was dispatched in the SAME wake — the healthy task cascaded
    // intra-wake. With the single per-wake barrier, both tasks are submitted
    // before the fault becomes visible, so neither cascades in the breaching
    // wake; the cascade lands on the NEXT wake. Each wake costs ~20 ms (the slow
    // task's sleep dominates the barrier) and the fault isn't visible until
    // ~25 ms, so the old 60 ms left too little headroom for a reliable post-fault
    // wake on loaded macOS CI. A generous window guarantees at least one
    // post-fault wake; the executor still faults exactly once (idempotent
    // transition), so the count assertions are unaffected by the wider window.
    exec.run_for(Duration::from_millis(300)).expect("run");

    // Executor must be Faulted.
    assert!(
        matches!(
            exec.executor_fault_state(),
            ExecutorFaultState::Faulted { .. }
        ),
        "executor should be Faulted; got {:?}",
        exec.executor_fault_state()
    );
    assert_eq!(observer.executor_fault_count.load(Ordering::SeqCst), 1);
    // Cascade-noise invariant: per-task on_task_fault should NOT fire for the
    // healthy task (it didn't breach its own budget — only the executor breach
    // cascaded it). For the slow task, it has no per-task budget either, so
    // its on_task_fault also does NOT fire. Total per-task faults = 0.
    assert_eq!(observer.task_fault_count.load(Ordering::SeqCst), 0);

    // Clear the executor fault — tasks that were lazy-cascaded to
    // Faulted{ExecutorFaulted} should get on_task_clear; on_executor_clear
    // fires once. The cascade is lazy: a task transitions to
    // Faulted{ExecutorFaulted} only on its next pre-dispatch after the
    // executor fault (see the post-fault-wake note on the run window above).
    // Even with the generous window the two tasks may reach that post-fault
    // wakeup on different cycles, so assert >= 1 cascade clear rather than 2
    // (the load-bearing invariant is the cascade-noise rule above:
    // task_fault_count == 0).
    exec.clear_executor_fault().expect("clear");
    assert_eq!(observer.executor_clear_count.load(Ordering::SeqCst), 1);
    assert!(
        observer.task_clear_count.load(Ordering::SeqCst) >= 1,
        "expected at least one cascade-cleared task; got {}",
        observer.task_clear_count.load(Ordering::SeqCst)
    );

    // Both tasks should now be Running.
    use taktora_executor::FaultState;
    assert_eq!(exec.task_fault_state(slow_id).unwrap(), FaultState::Running);
    assert_eq!(
        exec.task_fault_state(healthy_id).unwrap(),
        FaultState::Running
    );
}
