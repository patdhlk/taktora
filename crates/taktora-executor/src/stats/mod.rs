//! Executor-side telemetry value types (`REQ_0103`).
//!
//! Aggregation math lives in `taktora-stats` ([`ExecutorCycleStats`][ext]);
//! this module provides the std-side wiring carried across the [`Observer`]
//! boundary: the per-cycle push observation [`CycleObservation`] and the
//! pull snapshot [`StatsSnapshot`] / [`TaskStatsEntry`].
//!
//! [ext]: taktora_stats::ExecutorCycleStats
//! [`Observer`]: crate::Observer

use crate::TaskId;

/// A single per-task observation pushed by the executor each cycle.
///
/// One value is emitted per dispatched task per cycle and handed to the
/// observer (or buffered for aggregation). Because [`TaskId`] is `Arc<str>`
/// under the hood, [`CycleObservation`] is `Clone` but not `Copy`.
#[derive(Clone, Debug)]
pub struct CycleObservation {
    /// Monotonic cycle counter, advances on every dispatch attempt including
    /// faulted scans (`REQ_0107`).
    pub cycle_index: u64,

    /// Identifier of the task this observation belongs to.
    pub task_id: TaskId,

    /// Declared (nominal) scan period in nanoseconds.
    pub period_ns: u64,

    /// Measured period since the previous dispatch of this task in
    /// nanoseconds. Set to `0` on the first cycle or when the previous
    /// timestamp is not available (faulted scan).
    pub actual_period_ns: u64,

    /// Absolute jitter: `|actual_period_ns − period_ns|`. Set to `0` when
    /// not measurable (e.g. first cycle).
    pub jitter_ns: u64,

    /// Signed deadline lateness relative to the nominal dispatch grid in
    /// nanoseconds. Positive values mean late (`REQ_0106`).
    pub lateness_ns: i64,

    /// Wall-clock execution duration of the task in nanoseconds. Set to `0`
    /// on a faulted scan where the task body was not entered.
    pub took_ns: u64,
}

/// Aggregated statistics for a single task, produced by a pull snapshot.
///
/// Percentile fields (`p50_ns`, `p95_ns`, `p99_ns`) are estimates from the
/// `taktora-stats` rank-based histogram. `min_ns` and `max_ns` are exact
/// (`REQ_0105`).
#[derive(Clone, Debug)]
pub struct TaskStatsEntry {
    /// Identifier of the task these statistics belong to.
    pub task_id: TaskId,

    /// Estimated 50th-percentile execution duration in nanoseconds.
    pub p50_ns: u64,

    /// Estimated 95th-percentile execution duration in nanoseconds.
    pub p95_ns: u64,

    /// Estimated 99th-percentile execution duration in nanoseconds.
    pub p99_ns: u64,

    /// Exact minimum execution duration observed (`REQ_0105`).
    pub min_ns: u64,

    /// Exact maximum execution duration observed (`REQ_0105`).
    pub max_ns: u64,

    /// Peak jitter (maximum `|actual_period − period|`) observed (`REQ_0101`).
    pub max_jitter_ns: u64,

    /// Peak (unsigned) deadline lateness observed (`REQ_0106`).
    pub max_lateness_ns: u64,

    /// Number of times this task exceeded its execution deadline (`REQ_0102`),
    /// read from the per-task overrun counter.
    pub overrun_count: u64,
}

/// A point-in-time pull snapshot of executor telemetry (`REQ_0103`).
///
/// Contains one [`TaskStatsEntry`] per registered task in registration order.
/// The `Vec` allocation is on the caller's side; internal ring-buffer
/// accounting is out of scope for `REQ_0104`.
#[derive(Clone, Debug)]
pub struct StatsSnapshot {
    /// Per-task aggregated statistics, one entry per registered task in
    /// registration order.
    pub per_task: Vec<TaskStatsEntry>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TaskId;

    #[test]
    fn cycle_observation_is_clone_and_holds_all_fields() {
        let obs = CycleObservation {
            cycle_index: 3,
            task_id: TaskId::from("t0"),
            period_ns: 10_000_000,
            actual_period_ns: 10_050_000,
            jitter_ns: 50_000,
            lateness_ns: -120,
            took_ns: 1_000_000,
        };
        // Verify Clone is implemented and produces an independent copy.
        let copy = obs.clone();
        drop(obs);
        assert_eq!(copy.cycle_index, 3);
        assert_eq!(copy.task_id.as_str(), "t0");
        assert_eq!(copy.lateness_ns, -120);
    }

    #[test]
    fn stats_snapshot_holds_per_task_entries() {
        let snap = StatsSnapshot {
            per_task: vec![TaskStatsEntry {
                task_id: TaskId::from("t0"),
                p50_ns: 1,
                p95_ns: 2,
                p99_ns: 3,
                min_ns: 1,
                max_ns: 4,
                max_jitter_ns: 5,
                max_lateness_ns: 6,
                overrun_count: 7,
            }],
        };
        assert_eq!(snap.per_task.len(), 1);
        assert_eq!(snap.per_task[0].overrun_count, 7);
    }
}
