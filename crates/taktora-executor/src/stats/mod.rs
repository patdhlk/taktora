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
///
/// **Absent vs. zero (`REQ_0103`).** Every measured quantity is an
/// [`Option`]: `None` means "not measured this cycle", which is *not* the
/// same as a measured `0`. A faulted scan (see [`faulted`](Self::faulted)
/// and `REQ_0107`) advances `cycle_index` but enters no task body, so
/// `took_ns`/`jitter_ns`/`lateness_ns` are all `None`; `actual_period_ns`
/// is also `None` on the very first cycle. This mirrors the connector's
/// observation contract (`REQ_0267`), where a faulted wire round reports
/// `wire_round_ns: None` — so a consumer joining the two push streams on
/// `cycle_index` sees a consistent "absent on fault" signal from both
/// layers instead of an ambiguous `0`.
#[derive(Clone, Debug)]
pub struct CycleObservation {
    /// Monotonic cycle counter, advances on every dispatch attempt including
    /// faulted scans (`REQ_0107`).
    pub cycle_index: u64,

    /// Identifier of the task this observation belongs to.
    pub task_id: TaskId,

    /// Stable zero-based registration index of the task, assigned at
    /// `Executor::add` time and constant for the executor's lifetime
    /// (`REQ_0103`). The flat `u32` join/identity key for telemetry export
    /// (`REQ_0111`'s `task_id` column) — frees consumers from hashing the
    /// `Arc<str>` [`task_id`](Self::task_id) on the hot path.
    pub task_index: u32,

    /// `true` when this scan was fault-routed / skipped: the task body was
    /// not entered, so every measured field below is `None` (`REQ_0107`).
    /// The cross-layer twin of the connector's `CycleOutcome::Fault`
    /// (`REQ_0267`).
    pub faulted: bool,

    /// Declared (nominal) scan period in nanoseconds. Always known.
    pub period_ns: u64,

    /// Telemetry-clock nanosecond instant of **task-logic start** — the
    /// canonical reference point (`pre_execute`), the same instant the
    /// period/jitter/lateness folds are sampled against (`REQ_0103`,
    /// `REQ_0101`). The single time source for an exported sample's time
    /// axis; never a second clock read. Always populated.
    pub pre_ns: u64,

    /// Measured period since the previous dispatch of this task in
    /// nanoseconds. `None` on the first cycle (no previous timestamp).
    pub actual_period_ns: Option<u64>,

    /// Absolute jitter: `|actual_period_ns − period_ns|`. `None` when not
    /// measurable (first cycle) or on a faulted scan.
    pub jitter_ns: Option<u64>,

    /// Signed deadline lateness relative to the nominal dispatch grid in
    /// nanoseconds; positive means late (`REQ_0106`). `None` on a faulted
    /// scan or an event-driven task.
    pub lateness_ns: Option<i64>,

    /// Nominal grid slots the dispatcher passed over **unserved** between the
    /// slot served by this task's previous dispatch and the slot served by
    /// this one (the skip-realign of `REQ_0268`), per `REQ_0840`. Always
    /// present: `0` in steady state, always `0` in `Legacy` dispatch mode
    /// (which never skips slots) and on a task's first recorded cycle. The
    /// lateness grid of `REQ_0106` advances by exactly `1 + skipped_slots`.
    pub skipped_slots: u32,

    /// Wall-clock execution duration of the task in nanoseconds. `None` on
    /// a faulted scan (the body was not entered) or when no sample was
    /// recorded this cycle (e.g. a fault handler ran in the item's place).
    pub took_ns: Option<u64>,
}

/// Aggregated statistics for a single task, produced by a pull snapshot.
///
/// **Precision contract.** `min_ns`/`max_ns` are **exact** (`REQ_0105`) and
/// are the values to use for any threshold or regression decision. The
/// percentile fields (`p50_ns`, `p95_ns`, `p99_ns`) are octave-bucket
/// *estimates* from the `taktora-stats` histogram, carrying up to
/// [`PERCENTILE_MAX_REL_ERR_PCT`](taktora_stats::PERCENTILE_MAX_REL_ERR_PCT)
/// relative error — they locate the order of magnitude, not the exact
/// figure. (`REQ_0100`'s ≤ 1% target awaits a sub-octave histogram.)
#[derive(Clone, Debug)]
pub struct TaskStatsEntry {
    /// Identifier of the task these statistics belong to.
    pub task_id: TaskId,

    /// Estimated 50th-percentile execution duration in nanoseconds
    /// (octave-bucket estimate; see the struct-level precision contract).
    pub p50_ns: u64,

    /// Estimated 95th-percentile execution duration in nanoseconds
    /// (octave-bucket estimate; see the struct-level precision contract).
    pub p95_ns: u64,

    /// Estimated 99th-percentile execution duration in nanoseconds
    /// (octave-bucket estimate; see the struct-level precision contract).
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
            task_index: 0,
            faulted: false,
            period_ns: 10_000_000,
            pre_ns: 0,
            actual_period_ns: Some(10_050_000),
            jitter_ns: Some(50_000),
            lateness_ns: Some(-120),
            skipped_slots: 0,
            took_ns: Some(1_000_000),
        };
        // Verify Clone is implemented and produces an independent copy;
        // both original and copy are read so the clone is genuinely exercised.
        let copy = obs.clone();
        assert_eq!(obs.cycle_index, 3);
        assert_eq!(copy.cycle_index, obs.cycle_index);
        assert_eq!(copy.task_id.as_str(), "t0");
        assert_eq!(copy.lateness_ns, Some(-120));
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
