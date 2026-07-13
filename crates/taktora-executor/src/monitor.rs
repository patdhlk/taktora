//! `ExecutionMonitor` — pre/post `execute` timestamps.

use crate::task_id::TaskId;
use core::time::Duration;
use std::time::Instant;

/// Hook invoked before and after every `execute` call. Defaults are no-ops.
///
/// # Why two hooks?
///
/// This is the per-`execute` timing hook: it fires on the dispatch hot
/// path around every single item invocation, so implementations must stay
/// cheap. For coarse lifecycle events (executor up/down, app start/stop,
/// faults, user events) — which fire rarely and can afford more work —
/// use [`crate::Observer`] instead.
pub trait ExecutionMonitor: Send + Sync {
    /// Called immediately before an item's `execute()` is invoked.
    fn pre_execute(&self, _task: TaskId, _at: Instant) {}
    /// Called immediately after `execute()` returns. `ok` is `false` if the
    /// item returned `Err` (or panicked).
    fn post_execute(&self, _task: TaskId, _at: Instant, _took: Duration, _ok: bool) {}
}

/// No-op monitor used when the user does not configure one.
pub struct NoopMonitor;
impl ExecutionMonitor for NoopMonitor {}
