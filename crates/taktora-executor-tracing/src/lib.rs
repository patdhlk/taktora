//! `tracing`-based [`Observer`].
//!
//! Pass `Arc::new(TracingObserver::default())` to
//! [`ExecutorBuilder::observer`](taktora_executor::ExecutorBuilder)
//! to forward all executor lifecycle events to the global `tracing`
//! subscriber.

#![doc(html_root_url = "https://docs.rs/taktora-executor-tracing/0.1.0")]

use taktora_executor::{
    ExecutorError, ExecutorFaultReason, FaultReason, Observer, TaskId, UserEvent,
};

/// Observer that forwards every callback to the global `tracing` subscriber.
#[derive(Debug, Default)]
pub struct TracingObserver;

impl Observer for TracingObserver {
    fn on_executor_up(&self) {
        tracing::info!(target: "taktora.executor", "executor.up");
    }
    fn on_executor_down(&self) {
        tracing::info!(target: "taktora.executor", "executor.down");
    }
    fn on_executor_error(&self, e: &ExecutorError) {
        tracing::error!(target: "taktora.executor", error = %e, "executor.error");
    }

    fn on_app_start(&self, task: TaskId, app: u32, instance: Option<u32>) {
        tracing::info!(
            target: "taktora.app",
            task = %task,
            app,
            ?instance,
            "app.start"
        );
    }
    fn on_app_stop(&self, task: TaskId) {
        tracing::info!(target: "taktora.app", task = %task, "app.stop");
    }
    fn on_app_error(&self, task: TaskId, e: &(dyn std::error::Error + 'static)) {
        tracing::error!(
            target: "taktora.app",
            task = %task,
            error = %e,
            "app.error"
        );
    }

    fn on_send_event(&self, task: TaskId, ev: UserEvent) {
        tracing::event!(
            target: "taktora.user",
            tracing::Level::INFO,
            task = %task,
            kind = ev.kind,
            int_data = ev.int_data,
            string_data = ev.string_data.as_deref().unwrap_or(""),
            "user.event"
        );
    }

    fn on_task_fault(&self, task: TaskId, reason: FaultReason) {
        tracing::warn!(
            target: "taktora.fault",
            task = %task,
            ?reason,
            "task.fault"
        );
    }
    fn on_task_clear(&self, task: TaskId) {
        tracing::info!(target: "taktora.fault", task = %task, "task.clear");
    }
    fn on_executor_fault(&self, reason: ExecutorFaultReason) {
        tracing::warn!(target: "taktora.fault", ?reason, "executor.fault");
    }
    fn on_executor_clear(&self) {
        tracing::info!(target: "taktora.fault", "executor.clear");
    }
}
