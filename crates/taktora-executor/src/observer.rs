//! `Observer` trait — lifecycle hooks invoked by the executor.

use crate::error::ExecutorError;
use crate::fault::{ExecutorFaultReason, FaultReason};
use crate::heartbeat::HeartbeatTick;
use crate::stats::CycleObservation;
use crate::task_id::TaskId;

/// Generic user event carried by [`Observer::on_send_event`].
///
/// # Construction
///
/// Use [`UserEvent::new`] to create a value; struct literal syntax is not
/// available from outside this crate because `UserEvent` is `#[non_exhaustive]`.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct UserEvent {
    /// User-defined event kind.
    pub kind: u32,
    /// Numeric payload.
    pub int_data: i64,
    /// Optional string payload.
    pub string_data: Option<String>,
}

impl UserEvent {
    /// Create a new event with the given `kind` and `int_data`.
    #[must_use]
    pub const fn new(kind: u32, int_data: i64) -> Self {
        Self {
            kind,
            int_data,
            string_data: None,
        }
    }

    /// Attach an optional string payload to this event.
    #[must_use]
    pub fn with_string(mut self, s: impl Into<String>) -> Self {
        self.string_data = Some(s.into());
        self
    }
}

/// Lifecycle observer invoked by the executor at well-defined points.
///
/// # Why two hooks?
///
/// This handles coarse, infrequent lifecycle events — executor up/down,
/// app start/stop, faults, and user events — so its callbacks can afford
/// to do real work. For per-`execute` timing on the dispatch hot path
/// (which must stay cheap), use [`crate::ExecutionMonitor`] instead.
///
/// All methods have no-op defaults. The executor never blocks on observer
/// callbacks — heavy work should be queued internally.
pub trait Observer: Send + Sync {
    /// Called once just before the dispatch loop begins.
    fn on_executor_up(&self) {}
    /// Called once just after the dispatch loop finishes cleanly.
    fn on_executor_down(&self) {}
    /// Called when the dispatch loop returns an error.
    fn on_executor_error(&self, _e: &ExecutorError) {}

    /// Called before an item with `app_id().is_some()` runs (per invocation).
    fn on_app_start(&self, _task: TaskId, _app: u32, _instance: Option<u32>) {}
    /// Called after such an item runs.
    fn on_app_stop(&self, _task: TaskId) {}
    /// Called when an item returns `Err` or panics.
    fn on_app_error(&self, _task: TaskId, _e: &(dyn std::error::Error + 'static)) {}

    /// Called when an item invokes `Context::send_event`.
    fn on_send_event(&self, _task: TaskId, _ev: UserEvent) {}

    /// Called once when a task transitions from `Running` to `Faulted`
    /// (per-task budget overrun, `REQ_0070`). The cascade transition
    /// triggered by an executor-wide fault does NOT fire this hook —
    /// see [`Observer::on_executor_fault`]. `REQ_0073`.
    fn on_task_fault(&self, _task: TaskId, _reason: FaultReason) {}

    /// Called once when a task transitions from `Faulted` back to
    /// `Running` (manual clear via `Executor::clear_task_fault`).
    fn on_task_clear(&self, _task: TaskId) {}

    /// Called once when the executor transitions from `Running` to
    /// `Faulted` (executor-wide iteration budget breach, `REQ_0071`).
    fn on_executor_fault(&self, _reason: ExecutorFaultReason) {}

    /// Called once when the executor transitions from `Faulted` back
    /// to `Running` (manual clear via `Executor::clear_executor_fault`).
    fn on_executor_clear(&self) {}

    /// Fires once per scan cycle of a cyclic task, including a faulted scan
    /// (`REQ_0103`, `REQ_0107`). Default no-op for backward compatibility.
    ///
    /// **Containment:** runs on the executor's `WaitSet` thread outside the
    /// per-item panic catch — a panic here routes to the fail-fast boundary
    /// (`REQ_0123`). Implementations must not panic.
    fn on_cycle_stats(&self, _obs: &CycleObservation) {}

    /// Called on every heartbeat tick when the executor is configured with
    /// [`crate::ExecutorBuilder::heartbeat`].
    ///
    /// Emitted at a bounded period (≤ configured `period` under typical load)
    /// to signal executor liveness. Default no-op for backward compatibility.
    ///
    /// # Timing
    ///
    /// The executor guarantees at least one tick per configured period,
    /// regardless of other dispatch activity. The `WaitSet` wait is bounded by
    /// the heartbeat deadline. Alloc-free on the hot path (tick is `Copy`,
    /// passed by reference).
    ///
    /// # Containment
    ///
    /// Runs on the executor's `WaitSet` thread outside the per-item panic
    /// catch — a panic here routes to the fail-fast boundary (`REQ_0123`).
    /// Implementations must not panic.
    ///
    /// # TSR coverage
    ///
    /// Supports `TSR_0010` / `AOU_0003`: liveness heartbeat for external
    /// watchdog integration.
    fn on_heartbeat(&self, _tick: &HeartbeatTick) {}
}

/// No-op observer used when the user does not configure one.
pub struct NoopObserver;
impl Observer for NoopObserver {}

#[cfg(test)]
mod cycle_stats_hook_tests {
    use super::*;
    use crate::TaskId;
    use crate::stats::CycleObservation;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct CountingObs(Arc<AtomicU64>);
    impl Observer for CountingObs {
        fn on_cycle_stats(&self, _: &CycleObservation) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn sample_obs() -> CycleObservation {
        CycleObservation {
            cycle_index: 0,
            task_id: TaskId::from("t"),
            task_index: 0,
            faulted: false,
            period_ns: 0,
            pre_ns: 0,
            actual_period_ns: None,
            jitter_ns: None,
            lateness_ns: None,
            skipped_slots: 0,
            took_ns: None,
        }
    }

    #[test]
    fn default_on_cycle_stats_is_noop() {
        let noop = NoopObserver;
        noop.on_cycle_stats(&sample_obs()); // default no-op: must compile & not panic
    }

    #[test]
    fn overridden_on_cycle_stats_fires() {
        let n = Arc::new(AtomicU64::new(0));
        let c = CountingObs(Arc::clone(&n));
        c.on_cycle_stats(&sample_obs());
        assert_eq!(n.load(Ordering::Relaxed), 1);
    }
}
