//! TEST_0822 — `taktora-executor-tracing` forwards the four fault-state
//! `Observer` callbacks to `tracing::warn!` / `tracing::info!` with the
//! documented field shape (target: "taktora.fault", message:
//! "task.fault" / "task.clear" / "executor.fault" / "executor.clear").
//!
//! This test installs the adapter as the executor's `Observer`, triggers
//! a per-task budget overrun (Running -> Faulted{BudgetExceeded}), then
//! clears it (Faulted -> Running), and asserts the corresponding tracing
//! events fire on the documented target with the documented message.

#![allow(missing_docs)]

use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use taktora_executor::{
    ExecuteResult, Executor, ExecutorError, ItemFlow, Observer, item_with_triggers,
};
use taktora_executor_tracing::TracingObserver;
use tracing::Subscriber;
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::prelude::*;

#[derive(Default)]
struct CapturedEvent {
    target: String,
    message: String,
}

#[derive(Default)]
struct EventCollector {
    events: Mutex<Vec<CapturedEvent>>,
}

#[derive(Default)]
struct MessageVisitor {
    message: String,
}

impl tracing::field::Visit for MessageVisitor {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        }
    }
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{value:?}");
        }
    }
}

/// Layer wrapper that owns an `Arc<EventCollector>` so the test body can
/// inspect the captured events after `with_default` returns.
struct EventCollectorLayer {
    inner: Arc<EventCollector>,
}

impl<S> Layer<S> for EventCollectorLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let target = event.metadata().target().to_string();
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);
        // The `tracing::info!(..., "msg")` macro records the literal as a
        // `record_debug` on the `message` field, which formats as
        // `"msg"` (with surrounding quotes). Strip those so callers can
        // match on the bare literal.
        let message = visitor.message.trim_matches('"').to_string();
        self.inner
            .events
            .lock()
            .unwrap()
            .push(CapturedEvent { target, message });
    }
}

#[test]
fn fault_callbacks_forwarded_to_tracing() {
    let collector: Arc<EventCollector> = Arc::new(EventCollector::default());
    let collector_for_layer = Arc::clone(&collector);

    // Build a subscriber that pushes events into the collector.
    let subscriber = tracing_subscriber::registry().with(EventCollectorLayer {
        inner: collector_for_layer,
    });

    tracing::subscriber::with_default(subscriber, || {
        let observer: Arc<dyn Observer> = Arc::new(TracingObserver);

        // Inline mode (worker_threads = 0) keeps all dispatch — including
        // `post_execute_detect_fault`, which invokes `on_task_fault` — on
        // the calling thread, so the thread-local subscriber installed by
        // `with_default` actually receives the events. With a real worker
        // pool the spawned OS threads inherit the no-op global subscriber
        // and silently drop the events.
        let mut exec = Executor::builder()
            .worker_threads(0)
            .observer(observer)
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
                    // Sleep long enough to breach the 1ms budget on the
                    // first execute() call, transitioning the task to
                    // Faulted{BudgetExceeded} and firing on_task_fault.
                    std::thread::sleep(Duration::from_millis(3));
                    Ok(ItemFlow::Continue)
                },
            ))
            .expect("add");

        exec.run_for(Duration::from_millis(30)).expect("run");
        // Clear the per-task fault so on_task_clear fires.
        exec.clear_task_fault(task_id).expect("clear");
    });

    let events = collector.events.lock().unwrap();
    let event_summary = events
        .iter()
        .map(|e| format!("[{}] {}", e.target, e.message))
        .collect::<Vec<_>>()
        .join("\n");

    let has_task_fault = events
        .iter()
        .any(|e| e.target == "taktora.fault" && e.message == "task.fault");
    let has_task_clear = events
        .iter()
        .any(|e| e.target == "taktora.fault" && e.message == "task.clear");

    assert!(
        has_task_fault,
        "expected a 'task.fault' event on target 'taktora.fault'; got:\n{event_summary}"
    );
    assert!(
        has_task_clear,
        "expected a 'task.clear' event on target 'taktora.fault'; got:\n{event_summary}"
    );
}
