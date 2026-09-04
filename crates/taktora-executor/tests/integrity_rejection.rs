#![allow(missing_docs)]
use core::time::Duration;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use taktora_executor::{
    Context, Executor, ExecutorError, IntegrityLevel, ItemFlow, TriggerDeclarer, item_with_triggers,
};

// ── Test item that overrides integrity_level ─────────────────────────────────

struct TestItem {
    level: IntegrityLevel,
    counter: Arc<AtomicU32>,
}

impl taktora_executor::ExecutableItem for TestItem {
    fn declare_triggers(&mut self, d: &mut TriggerDeclarer<'_>) -> Result<(), ExecutorError> {
        d.interval(Duration::from_millis(10));
        Ok(())
    }

    fn execute(&mut self, _ctx: &mut Context<'_>) -> taktora_executor::ExecuteResult {
        self.counter.fetch_add(1, Ordering::SeqCst);
        Ok(ItemFlow::Continue)
    }

    fn integrity_level(&self) -> IntegrityLevel {
        self.level
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[test]
fn pinned_safety_critical_rejects_quality_managed() {
    let mut exec = Executor::builder()
        .worker_threads(0)
        .integrity_level(IntegrityLevel::SafetyCritical)
        .build()
        .unwrap();

    let counter = Arc::new(AtomicU32::new(0));
    let item = TestItem {
        level: IntegrityLevel::QualityManaged,
        counter,
    };

    let result = exec.add(item);
    assert!(matches!(
        result,
        Err(ExecutorError::MixedIntegrity {
            expected: IntegrityLevel::SafetyCritical,
            found: IntegrityLevel::QualityManaged,
        })
    ));
}

#[test]
fn pinned_safety_critical_accepts_safety_critical() {
    let mut exec = Executor::builder()
        .worker_threads(0)
        .integrity_level(IntegrityLevel::SafetyCritical)
        .build()
        .unwrap();

    let counter = Arc::new(AtomicU32::new(0));
    let c = Arc::clone(&counter);
    let item = TestItem {
        level: IntegrityLevel::SafetyCritical,
        counter: c,
    };

    exec.add(item).unwrap();
    exec.run_n(2).unwrap();
    assert_eq!(counter.load(Ordering::SeqCst), 2);
}

#[test]
fn unpinned_executor_accepts_quality_managed() {
    let mut exec = Executor::builder().worker_threads(0).build().unwrap();

    let counter = Arc::new(AtomicU32::new(0));
    let c = Arc::clone(&counter);
    let item = TestItem {
        level: IntegrityLevel::QualityManaged,
        counter: c,
    };

    exec.add(item).unwrap();
    exec.run_n(2).unwrap();
    assert_eq!(counter.load(Ordering::SeqCst), 2);
}

#[test]
fn unpinned_executor_accepts_mixed_levels() {
    let mut exec = Executor::builder().worker_threads(0).build().unwrap();

    let counter1 = Arc::new(AtomicU32::new(0));
    let c1 = Arc::clone(&counter1);
    let item1 = TestItem {
        level: IntegrityLevel::QualityManaged,
        counter: c1,
    };

    let counter2 = Arc::new(AtomicU32::new(0));
    let c2 = Arc::clone(&counter2);
    let item2 = TestItem {
        level: IntegrityLevel::SafetyCritical,
        counter: c2,
    };

    exec.add(item1).unwrap();
    exec.add(item2).unwrap();
    exec.run_n(2).unwrap();

    assert_eq!(counter1.load(Ordering::SeqCst), 2);
    assert_eq!(counter2.load(Ordering::SeqCst), 2);
}

#[test]
fn chain_rejects_mixed_integrity() {
    let mut exec = Executor::builder()
        .worker_threads(0)
        .integrity_level(IntegrityLevel::SafetyCritical)
        .build()
        .unwrap();

    let counter1 = Arc::new(AtomicU32::new(0));
    let counter2 = Arc::new(AtomicU32::new(0));

    let item1 = TestItem {
        level: IntegrityLevel::SafetyCritical,
        counter: counter1,
    };
    let item2 = TestItem {
        level: IntegrityLevel::QualityManaged,
        counter: counter2,
    };

    let result = exec.add_chain(vec![item1, item2]);
    assert!(matches!(
        result,
        Err(ExecutorError::MixedIntegrity {
            expected: IntegrityLevel::SafetyCritical,
            found: IntegrityLevel::QualityManaged,
        })
    ));
}

#[test]
fn chain_accepts_matching_integrity() {
    let mut exec = Executor::builder()
        .worker_threads(0)
        .integrity_level(IntegrityLevel::SafetyCritical)
        .build()
        .unwrap();

    let counter1 = Arc::new(AtomicU32::new(0));
    let c1 = Arc::clone(&counter1);
    let counter2 = Arc::new(AtomicU32::new(0));
    let c2 = Arc::clone(&counter2);

    let item1 = TestItem {
        level: IntegrityLevel::SafetyCritical,
        counter: c1,
    };
    let item2 = TestItem {
        level: IntegrityLevel::SafetyCritical,
        counter: c2,
    };

    exec.add_chain(vec![item1, item2]).unwrap();
    exec.run_n(2).unwrap();

    // Both items in chain execute
    assert_eq!(counter1.load(Ordering::SeqCst), 2);
    assert_eq!(counter2.load(Ordering::SeqCst), 2);
}

#[test]
fn graph_rejects_mixed_integrity() {
    let mut exec = Executor::builder()
        .worker_threads(0)
        .integrity_level(IntegrityLevel::SafetyCritical)
        .build()
        .unwrap();

    let counter1 = Arc::new(AtomicU32::new(0));
    let counter2 = Arc::new(AtomicU32::new(0));

    let item1 = TestItem {
        level: IntegrityLevel::SafetyCritical,
        counter: counter1,
    };
    let item2 = TestItem {
        level: IntegrityLevel::QualityManaged,
        counter: counter2,
    };

    let mut builder = exec.add_graph();
    let v1 = builder.vertex(item1);
    let v2 = builder.vertex(item2);
    builder.edge(v1, v2);
    builder.root(v1);

    let result = builder.build();
    assert!(matches!(
        result,
        Err(ExecutorError::MixedIntegrity {
            expected: IntegrityLevel::SafetyCritical,
            found: IntegrityLevel::QualityManaged,
        })
    ));
}

#[test]
fn graph_accepts_matching_integrity() {
    let mut exec = Executor::builder()
        .worker_threads(0)
        .integrity_level(IntegrityLevel::SafetyCritical)
        .build()
        .unwrap();

    let counter1 = Arc::new(AtomicU32::new(0));
    let c1 = Arc::clone(&counter1);
    let counter2 = Arc::new(AtomicU32::new(0));
    let c2 = Arc::clone(&counter2);

    let item1 = TestItem {
        level: IntegrityLevel::SafetyCritical,
        counter: c1,
    };
    let item2 = TestItem {
        level: IntegrityLevel::SafetyCritical,
        counter: c2,
    };

    let mut builder = exec.add_graph();
    let v1 = builder.vertex(item1);
    let v2 = builder.vertex(item2);
    builder.edge(v1, v2);
    builder.root(v1);

    builder.build().unwrap();
    exec.run_n(2).unwrap();

    // Both vertices execute
    assert_eq!(counter1.load(Ordering::SeqCst), 2);
    assert_eq!(counter2.load(Ordering::SeqCst), 2);
}

#[test]
fn default_item_integrity_is_quality_managed() {
    let mut exec = Executor::builder()
        .worker_threads(0)
        .integrity_level(IntegrityLevel::QualityManaged)
        .build()
        .unwrap();

    let counter = Arc::new(AtomicU32::new(0));
    let c = Arc::clone(&counter);

    // Default closure item uses default QualityManaged
    exec.add(item_with_triggers(
        |d| {
            d.interval(Duration::from_millis(10));
            Ok(())
        },
        move |_| {
            c.fetch_add(1, Ordering::SeqCst);
            Ok(ItemFlow::Continue)
        },
    ))
    .unwrap();

    exec.run_n(2).unwrap();
    assert_eq!(counter.load(Ordering::SeqCst), 2);
}

#[test]
fn pinned_quality_managed_rejects_safety_critical() {
    let mut exec = Executor::builder()
        .worker_threads(0)
        .integrity_level(IntegrityLevel::QualityManaged)
        .build()
        .unwrap();

    let counter = Arc::new(AtomicU32::new(0));
    let item = TestItem {
        level: IntegrityLevel::SafetyCritical,
        counter,
    };

    let result = exec.add(item);
    assert!(matches!(
        result,
        Err(ExecutorError::MixedIntegrity {
            expected: IntegrityLevel::QualityManaged,
            found: IntegrityLevel::SafetyCritical,
        })
    ));
}
