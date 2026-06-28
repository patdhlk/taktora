//! `TEST_0913` — a real `taktora-executor` running tasks drives the binding:
//! App entity health reflects the lifecycle hooks (start / stop / error) and the
//! readable `data` timing updates from `post_execute` (`REQ_0923`, `REQ_0924`).

#![allow(missing_docs)]

use core::time::Duration;
use std::sync::Arc;

use std::io;
use taktora_executor::{
    Context, ControlFlow, ExecutableItem, ExecuteResult, ExecutionMonitor, Executor, ExecutorError,
    Observer, TriggerDeclarer,
};
use taktora_medkit_binding_executor::ExecutorBinding;
use taktora_medkit_model::Health;
use taktora_medkit_provider::Provider;

/// A cyclic App item with a stable task id and an `app_id` (so the per-app
/// lifecycle hooks fire). When `fail` is set, `execute` returns `Err` to drive
/// the error path.
struct AppItem {
    id: &'static str,
    fail: bool,
}

impl ExecutableItem for AppItem {
    fn declare_triggers(&mut self, d: &mut TriggerDeclarer<'_>) -> Result<(), ExecutorError> {
        d.interval(Duration::from_millis(1));
        Ok(())
    }

    fn execute(&mut self, _ctx: &mut Context<'_>) -> ExecuteResult {
        if self.fail {
            Err(Box::new(io::Error::other("deliberate failure")))
        } else {
            Ok(ControlFlow::Continue)
        }
    }

    fn task_id(&self) -> Option<&str> {
        Some(self.id)
    }

    fn app_id(&self) -> Option<u32> {
        Some(7)
    }
}

fn build(binding: &Arc<ExecutorBinding>) -> Executor {
    Executor::builder()
        .worker_threads(0)
        .observer(Arc::clone(binding) as Arc<dyn Observer>)
        .monitor(Arc::clone(binding) as Arc<dyn ExecutionMonitor>)
        .build()
        .unwrap()
}

#[test]
fn running_task_is_healthy_and_times_update() {
    let binding = Arc::new(ExecutorBinding::with_tasks(["ctrl"]));
    let mut exec = build(&binding);
    exec.add(AppItem {
        id: "ctrl",
        fail: false,
    })
    .unwrap();

    exec.run_n(3).unwrap();

    // Liveness: the App started and stopped cleanly each cycle, no error.
    assert_eq!(binding.health("app:ctrl"), Health::Ok);
    assert_eq!(binding.health("executor"), Health::Ok);

    let snap = binding.snapshot();
    let live = &snap.data["app:ctrl"]["liveness"];
    assert!(live["starts"].as_u64().unwrap() >= 3, "starts: {live}");
    assert!(live["stops"].as_u64().unwrap() >= 3, "stops: {live}");
    assert_eq!(live["errors"], 0);

    // Timing folded from post_execute.
    let timing = &snap.data["app:ctrl"]["timing"];
    assert!(
        timing["executions"].as_u64().unwrap() >= 3,
        "executions: {timing}"
    );
    assert!(timing["ewma_took_ns"].as_u64().is_some());

    // Executor liveness data is present.
    assert_eq!(snap.data["executor"]["executor"]["task_count"], 1);
}

#[test]
fn erroring_task_degrades_to_error_health() {
    let binding = Arc::new(ExecutorBinding::with_tasks(["boom"]));
    let mut exec = build(&binding);
    exec.add(AppItem {
        id: "boom",
        fail: true,
    })
    .unwrap();

    // The item errors, so run surfaces the first ExecutorError — that is the
    // point: the binding observed the error through the hooks.
    let _ = exec.run_n(1);

    assert_eq!(binding.health("app:boom"), Health::Error);
    let snap = binding.snapshot();
    assert!(
        snap.data["app:boom"]["liveness"]["errors"]
            .as_u64()
            .unwrap()
            >= 1
    );
}
