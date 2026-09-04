#![allow(
    clippy::doc_markdown,
    clippy::field_reassign_with_default,
    clippy::items_after_statements
)]
#![allow(missing_docs)]
use core::time::Duration;
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use taktora_executor::{
    AdmissionFault, AdmissionOutcome, Context, Executor, ExecutorError, IntegrityLevel, ItemFlow,
    Observer, TriggerDeclarer, item_with_triggers,
};

/// Safety-critical item used to exercise a `SafetyCritical`-pinned executor
/// (its declared level matches the pin so `add` is accepted).
struct ScItem;

impl taktora_executor::ExecutableItem for ScItem {
    fn declare_triggers(&mut self, d: &mut TriggerDeclarer<'_>) -> Result<(), ExecutorError> {
        d.interval(Duration::from_millis(10));
        Ok(())
    }

    fn execute(&mut self, _ctx: &mut Context<'_>) -> taktora_executor::ExecuteResult {
        Ok(ItemFlow::Continue)
    }

    fn integrity_level(&self) -> IntegrityLevel {
        IntegrityLevel::SafetyCritical
    }
}

/// Test observer that captures admission events.
#[derive(Clone, Default)]
struct AdmissionObserver {
    admitted: Arc<AtomicBool>,
    rejected: Arc<AtomicBool>,
}

impl Observer for AdmissionObserver {
    fn on_admission_admitted(&self) {
        self.admitted.store(true, Ordering::SeqCst);
    }

    fn on_admission_rejected(&self, _fault: &AdmissionFault) {
        self.rejected.store(true, Ordering::SeqCst);
    }
}

#[test]
fn admission_rejected_prevents_dispatch_and_fires_observer() {
    let obs = AdmissionObserver::default();
    let obs_clone = obs.clone();

    let mut exec = Executor::builder()
        .worker_threads(0)
        .observer(Arc::new(obs))
        .admission_check(|_ctx| {
            AdmissionOutcome::Rejected(AdmissionFault::new("spatial isolation failed"))
        })
        .build()
        .unwrap();

    let counter = Arc::new(AtomicU32::new(0));
    let c = Arc::clone(&counter);

    exec.add(item_with_triggers(
        |d| {
            d.interval(Duration::from_millis(20));
            Ok(())
        },
        move |_| {
            c.fetch_add(1, Ordering::SeqCst);
            Ok(ItemFlow::Continue)
        },
    ))
    .unwrap();

    let result = exec.run_n(3);

    // Verify the error is AdmissionRejected.
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        ExecutorError::AdmissionRejected { .. }
    ));

    // Verify no task executed.
    assert_eq!(counter.load(Ordering::SeqCst), 0);

    // Verify on_admission_rejected was called.
    assert!(obs_clone.rejected.load(Ordering::SeqCst));
    assert!(!obs_clone.admitted.load(Ordering::SeqCst));
}

#[test]
fn admission_admitted_proceeds_normally_and_fires_observer() {
    let obs = AdmissionObserver::default();
    let obs_clone = obs.clone();

    let mut exec = Executor::builder()
        .worker_threads(0)
        .observer(Arc::new(obs))
        .admission_check(|_ctx| AdmissionOutcome::Admitted)
        .build()
        .unwrap();

    let counter = Arc::new(AtomicU32::new(0));
    let c = Arc::clone(&counter);

    exec.add(item_with_triggers(
        |d| {
            d.interval(Duration::from_millis(20));
            Ok(())
        },
        move |_| {
            c.fetch_add(1, Ordering::SeqCst);
            Ok(ItemFlow::Continue)
        },
    ))
    .unwrap();

    exec.run_n(3).unwrap();

    // Verify task executed 3 times.
    assert_eq!(counter.load(Ordering::SeqCst), 3);

    // Verify on_admission_admitted was called.
    assert!(obs_clone.admitted.load(Ordering::SeqCst));
    assert!(!obs_clone.rejected.load(Ordering::SeqCst));
}

#[test]
fn no_admission_check_runs_normally() {
    let mut exec = Executor::builder().worker_threads(0).build().unwrap();

    let counter = Arc::new(AtomicU32::new(0));
    let c = Arc::clone(&counter);

    exec.add(item_with_triggers(
        |d| {
            d.interval(Duration::from_millis(20));
            Ok(())
        },
        move |_| {
            c.fetch_add(1, Ordering::SeqCst);
            Ok(ItemFlow::Continue)
        },
    ))
    .unwrap();

    exec.run_n(3).unwrap();

    // Verify task executed 3 times (default path unchanged).
    assert_eq!(counter.load(Ordering::SeqCst), 3);
}

#[test]
fn admission_context_exposes_integrity_level() {
    let captured_level = Arc::new(Mutex::new(None));
    let level_clone = Arc::clone(&captured_level);

    let mut exec = Executor::builder()
        .worker_threads(0)
        .integrity_level(IntegrityLevel::SafetyCritical)
        .admission_check(move |ctx| {
            *level_clone.lock() = ctx.integrity_level();
            AdmissionOutcome::Admitted
        })
        .build()
        .unwrap();

    exec.add(ScItem).unwrap();

    exec.run_n(1).unwrap();

    // Verify the admission context saw the pinned integrity level.
    let level = *captured_level.lock();
    assert_eq!(level, Some(IntegrityLevel::SafetyCritical));
}

#[test]
fn admission_context_exposes_task_count() {
    let captured_count = Arc::new(Mutex::new(0));
    let count_clone = Arc::clone(&captured_count);

    let mut exec = Executor::builder()
        .worker_threads(0)
        .admission_check(move |ctx| {
            *count_clone.lock() = ctx.task_count();
            AdmissionOutcome::Admitted
        })
        .build()
        .unwrap();

    // Add two tasks.
    exec.add(item_with_triggers(
        |d| {
            d.interval(Duration::from_millis(20));
            Ok(())
        },
        move |_| Ok(ItemFlow::Continue),
    ))
    .unwrap();

    exec.add(item_with_triggers(
        |d| {
            d.interval(Duration::from_millis(20));
            Ok(())
        },
        move |_| Ok(ItemFlow::Continue),
    ))
    .unwrap();

    exec.run_n(1).unwrap();

    // Verify the admission context saw the task count.
    let count = *captured_count.lock();
    assert_eq!(count, 2);
}

#[test]
fn verify_isolation_default_admits() {
    let mut exec = Executor::builder()
        .worker_threads(0)
        .admission_check(taktora_executor::AdmissionContext::verify_isolation)
        .build()
        .unwrap();

    let counter = Arc::new(AtomicU32::new(0));
    let c = Arc::clone(&counter);

    exec.add(item_with_triggers(
        |d| {
            d.interval(Duration::from_millis(20));
            Ok(())
        },
        move |_| {
            c.fetch_add(1, Ordering::SeqCst);
            Ok(ItemFlow::Continue)
        },
    ))
    .unwrap();

    exec.run_n(2).unwrap();

    // Verify the default verification admitted and tasks executed.
    assert_eq!(counter.load(Ordering::SeqCst), 2);
}
