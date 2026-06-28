//! taktora-executor binding for `taktora-medkit`.
//!
//! This crate is one of the two seams where taktora coupling is quarantined
//! (`ADR_0111`). It implements the taktora-executor
//! [`Observer`] and
//! [`ExecutionMonitor`] hooks, records App
//! and executor liveness plus per-task timing into a **bounded, pre-allocated,
//! lock-free** sink, and exposes that sink to the medkit gateway through the
//! [`Provider`] seam — reading on the gateway's cadence, off the control path
//! (`REQ_0923`, `REQ_0924`, `REQ_0925`).
//!
//! # Freedom from interference (`ADR_0111`, `ADR_0114`)
//!
//! The executor invokes the hooks on its `WaitSet` thread, inside the
//! bounded-time control path. The write path therefore performs **no heap
//! allocation** and takes **no lock**: every hook resolves its task to a
//! pre-allocated slot and folds the observation into per-slot atomics with
//! single-writer relaxed stores. The provider read path runs on the gateway's
//! own (tokio) runtime and only reads those atomics, allocating the snapshot
//! there — never on the control path.
//!
//! Because the hooks fire from the single `WaitSet` thread, the sink is a
//! single-producer / single-consumer structure: the producer never contends and
//! never waits, so a stalled or slow reader can never perturb the machine.
//!
//! # Registration
//!
//! Wrap the binding in an [`Arc`](std::sync::Arc) and hand the same instance to
//! the executor builder as both the observer and the monitor, and to the gateway
//! as the [`Provider`]:
//!
//! ```no_run
//! use std::sync::Arc;
//! use taktora_executor::{Executor, ExecutionMonitor, Observer};
//! use taktora_medkit_binding_executor::ExecutorBinding;
//!
//! let binding = Arc::new(ExecutorBinding::with_tasks(["ctrl", "io"]));
//! let exec = Executor::builder()
//!     .observer(Arc::clone(&binding) as Arc<dyn Observer>)
//!     .monitor(Arc::clone(&binding) as Arc<dyn ExecutionMonitor>)
//!     .build()
//!     .unwrap();
//! # let _ = exec;
//! ```
//!
//! Tasks must be registered up front (the slots are pre-allocated so the hook
//! path never allocates): an item is correlated to a slot by its
//! [`TaskId`], which the executor sets from the item's
//! `task_id()`. Observations for an unregistered task are counted and dropped,
//! never allocated for.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use taktora_executor::{CycleObservation, ExecutionMonitor, Observer, TaskId, UserEvent};
use taktora_medkit_model::{Entity, EntityKind, EntityMeta, FaultSummary, Health};
use taktora_medkit_provider::{Provider, ProviderSnapshot};

/// Stable entity id of the synthetic executor entity emitted by the binding.
const EXECUTOR_ID: &str = "executor";

/// Convert a nominal scan period (ns) into a rate in Hz (the Hz analog), or
/// `None` when no period has been observed. Off the control path, so the
/// `u64`→`f64` widening (lossy only above 2^52 ns ≈ 52 days) is acceptable.
#[allow(clippy::cast_precision_loss)]
fn rate_hz(period_ns: u64) -> Option<f64> {
    (period_ns != 0).then(|| 1_000_000_000_f64 / period_ns as f64)
}

/// Weight (as a right-shift) of the integer EWMA over per-task execution
/// duration: `new = old − old/8 + sample/8`. A shift keeps the fold a few
/// integer ops with no division on the control path.
const EWMA_SHIFT: u32 = 3;

/// Liveness of a registered App, folded from the lifecycle hooks. Stored as a
/// [`u8`] in an atomic; see [`TaskSlot::state`].
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum Liveness {
    /// No lifecycle event observed yet.
    Unknown,
    /// Last event was `on_app_start`: the App is running.
    Running,
    /// Last event was `on_app_stop`: the App ran and stopped cleanly.
    Stopped,
    /// Last event was `on_app_error`: the App's item returned `Err` or panicked.
    Error,
}

impl Liveness {
    const fn as_u8(self) -> u8 {
        match self {
            Self::Unknown => 0,
            Self::Running => 1,
            Self::Stopped => 2,
            Self::Error => 3,
        }
    }

    const fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Running,
            2 => Self::Stopped,
            3 => Self::Error,
            _ => Self::Unknown,
        }
    }

    /// The directly-observed [`Health`] this liveness implies. A clean stop is
    /// not a fault (the App is simply offline); only an error degrades health.
    const fn health(self) -> Health {
        match self {
            Self::Error => Health::Error,
            _ => Health::Ok,
        }
    }

    /// Lowercase wire label surfaced under the entity's readable `data`.
    const fn label(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Running => "running",
            Self::Stopped => "stopped",
            Self::Error => "error",
        }
    }
}

/// Pre-allocated per-task sink slot. Every field is an atomic written only by
/// the single `WaitSet` thread (relaxed stores) and read by the gateway thread,
/// so the hook path neither allocates nor locks (`ADR_0114`).
#[derive(Debug)]
struct TaskSlot {
    /// Current [`Liveness`], as [`Liveness::as_u8`].
    state: AtomicU8,
    /// Last `app_id` seen from `on_app_start`.
    app_id: AtomicU32,
    /// Count of `on_app_start` calls.
    starts: AtomicU64,
    /// Count of `on_app_stop` calls.
    stops: AtomicU64,
    /// Count of `on_app_error` calls.
    errors: AtomicU64,
    /// Count of `post_execute` calls (folded timing samples).
    executions: AtomicU64,
    /// Most recent execution duration in nanoseconds (`post_execute`).
    last_took_ns: AtomicU64,
    /// Integer EWMA of execution duration in nanoseconds (the latency analog).
    ewma_took_ns: AtomicU64,
    /// Exact minimum execution duration observed, in nanoseconds.
    min_took_ns: AtomicU64,
    /// Exact maximum execution duration observed, in nanoseconds.
    max_took_ns: AtomicU64,
    /// Most recent nominal scan period in nanoseconds (`on_cycle_stats`); the
    /// basis for the Hz analog. `0` until a cyclic observation arrives.
    period_ns: AtomicU64,
}

impl TaskSlot {
    const fn new() -> Self {
        Self {
            state: AtomicU8::new(Liveness::Unknown.as_u8()),
            app_id: AtomicU32::new(0),
            starts: AtomicU64::new(0),
            stops: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            executions: AtomicU64::new(0),
            last_took_ns: AtomicU64::new(0),
            ewma_took_ns: AtomicU64::new(0),
            min_took_ns: AtomicU64::new(u64::MAX),
            max_took_ns: AtomicU64::new(0),
            period_ns: AtomicU64::new(0),
        }
    }

    /// Fold one execution-duration sample. Single-writer: load + store is sound
    /// without a compare-exchange, and nothing here touches the heap.
    fn record_took(&self, took: Duration) {
        let took_ns = u64::try_from(took.as_nanos()).unwrap_or(u64::MAX);
        let prior = self.executions.fetch_add(1, Ordering::Relaxed);
        self.last_took_ns.store(took_ns, Ordering::Relaxed);

        if took_ns < self.min_took_ns.load(Ordering::Relaxed) {
            self.min_took_ns.store(took_ns, Ordering::Relaxed);
        }
        if took_ns > self.max_took_ns.load(Ordering::Relaxed) {
            self.max_took_ns.store(took_ns, Ordering::Relaxed);
        }

        let ewma = if prior == 0 {
            took_ns
        } else {
            let old = self.ewma_took_ns.load(Ordering::Relaxed);
            old - (old >> EWMA_SHIFT) + (took_ns >> EWMA_SHIFT)
        };
        self.ewma_took_ns.store(ewma, Ordering::Relaxed);
    }
}

/// Binds taktora-executor lifecycle and timing hooks into the medkit
/// [`Provider`] seam, off the control path.
///
/// Construct with [`with_tasks`](Self::with_tasks) (the task ids whose Apps the
/// binding tracks), register it as the executor's observer and monitor, and read
/// it through the [`Provider`] seam. See the crate-level docs for the wiring and
/// the freedom-from-interference contract.
#[derive(Debug)]
pub struct ExecutorBinding {
    /// `on_executor_up` has fired and `on_executor_down` has not.
    executor_up: AtomicBool,
    /// The executor is in its `Faulted` state (`on_executor_fault`).
    executor_faulted: AtomicBool,
    /// Task id → slot index, built once at construction; read-only thereafter,
    /// so a lookup on the hook path neither locks nor allocates.
    index: HashMap<Box<str>, usize>,
    /// Pre-allocated per-task slots, parallel to `task_ids`.
    slots: Box<[TaskSlot]>,
    /// Registered task ids, in registration order (entity emission order).
    task_ids: Box<[Box<str>]>,
}

impl ExecutorBinding {
    /// Create a binding tracking the given task ids.
    ///
    /// One slot is pre-allocated per id, so the hook path never allocates. Ids
    /// must match the [`TaskId`] the executor assigns
    /// each item (from the item's `task_id()`); duplicates collapse to the first
    /// slot.
    #[must_use]
    pub fn with_tasks<I, S>(tasks: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut index = HashMap::new();
        let mut slots = Vec::new();
        let mut task_ids = Vec::new();
        for task in tasks {
            let id: Box<str> = task.into().into_boxed_str();
            if index.contains_key(&id) {
                continue;
            }
            index.insert(id.clone(), slots.len());
            slots.push(TaskSlot::new());
            task_ids.push(id);
        }
        Self {
            executor_up: AtomicBool::new(false),
            executor_faulted: AtomicBool::new(false),
            index,
            slots: slots.into_boxed_slice(),
            task_ids: task_ids.into_boxed_slice(),
        }
    }

    /// Create a binding tracking no tasks (executor liveness only).
    #[must_use]
    pub fn new() -> Self {
        Self::with_tasks(Vec::<String>::new())
    }

    /// Resolve a task to its pre-allocated slot without allocating. Returns
    /// `None` for an unregistered task.
    fn slot(&self, task: &TaskId) -> Option<&TaskSlot> {
        self.index.get(task.as_str()).map(|&i| &self.slots[i])
    }

    /// The SOVD entity id for a registered task's App (`app:<task>`).
    fn app_entity_id(task: &str) -> String {
        format!("app:{task}")
    }

    /// Build the readable `data` tree for one App slot (off the control path).
    fn app_data(slot: &TaskSlot) -> Value {
        let state = Liveness::from_u8(slot.state.load(Ordering::Relaxed));
        let executions = slot.executions.load(Ordering::Relaxed);
        let period_ns = slot.period_ns.load(Ordering::Relaxed);
        let min_ns = slot.min_took_ns.load(Ordering::Relaxed);
        json!({
            "liveness": {
                "state": state.label(),
                "online": state == Liveness::Running,
                "starts": slot.starts.load(Ordering::Relaxed),
                "stops": slot.stops.load(Ordering::Relaxed),
                "errors": slot.errors.load(Ordering::Relaxed),
            },
            "timing": {
                "executions": executions,
                "last_took_ns": slot.last_took_ns.load(Ordering::Relaxed),
                "ewma_took_ns": slot.ewma_took_ns.load(Ordering::Relaxed),
                "min_took_ns": if executions == 0 { 0 } else { min_ns },
                "max_took_ns": slot.max_took_ns.load(Ordering::Relaxed),
                "period_ns": period_ns,
                "rate_hz": rate_hz(period_ns).map_or(Value::Null, |hz| json!(hz)),
            },
        })
    }

    /// The synthetic executor entity (kind Component), carrying its liveness.
    fn executor_entity(&self) -> Entity {
        Entity {
            href: format!("/api/v1/components/{EXECUTOR_ID}"),
            id: EXECUTOR_ID.to_owned(),
            name: EXECUTOR_ID.to_owned(),
            kind: EntityKind::Component,
            parent_id: None,
            description: None,
            x_medkit: Some(EntityMeta {
                is_online: Some(self.executor_up.load(Ordering::Relaxed)),
                ..EntityMeta::default()
            }),
        }
    }

    /// The App entity for a registered task, carrying its online flag.
    fn app_entity(task: &str, slot: &TaskSlot) -> Entity {
        let id = Self::app_entity_id(task);
        let online = Liveness::from_u8(slot.state.load(Ordering::Relaxed)) == Liveness::Running;
        Entity {
            href: format!("/api/v1/apps/{id}"),
            id,
            name: task.to_owned(),
            kind: EntityKind::App,
            parent_id: None,
            description: None,
            x_medkit: Some(EntityMeta {
                is_online: Some(online),
                ..EntityMeta::default()
            }),
        }
    }
}

impl Default for ExecutorBinding {
    fn default() -> Self {
        Self::new()
    }
}

// ── Hook write path (control-path: no heap, no lock) ─────────────────────────

impl Observer for ExecutorBinding {
    fn on_executor_up(&self) {
        self.executor_up.store(true, Ordering::Relaxed);
    }

    fn on_executor_down(&self) {
        self.executor_up.store(false, Ordering::Relaxed);
    }

    fn on_executor_error(&self, _e: &taktora_executor::ExecutorError) {
        self.executor_up.store(false, Ordering::Relaxed);
        self.executor_faulted.store(true, Ordering::Relaxed);
    }

    fn on_executor_fault(&self, _reason: taktora_executor::ExecutorFaultReason) {
        self.executor_faulted.store(true, Ordering::Relaxed);
    }

    fn on_executor_clear(&self) {
        self.executor_faulted.store(false, Ordering::Relaxed);
    }

    fn on_app_start(&self, task: TaskId, app: u32, _instance: Option<u32>) {
        if let Some(slot) = self.slot(&task) {
            slot.app_id.store(app, Ordering::Relaxed);
            slot.starts.fetch_add(1, Ordering::Relaxed);
            slot.state
                .store(Liveness::Running.as_u8(), Ordering::Relaxed);
        }
    }

    fn on_app_stop(&self, task: TaskId) {
        if let Some(slot) = self.slot(&task) {
            slot.stops.fetch_add(1, Ordering::Relaxed);
            // A clean stop never downgrades an error already latched this cycle.
            let _ = slot.state.compare_exchange(
                Liveness::Running.as_u8(),
                Liveness::Stopped.as_u8(),
                Ordering::Relaxed,
                Ordering::Relaxed,
            );
        }
    }

    fn on_app_error(&self, task: TaskId, _e: &(dyn std::error::Error + 'static)) {
        if let Some(slot) = self.slot(&task) {
            slot.errors.fetch_add(1, Ordering::Relaxed);
            slot.state.store(Liveness::Error.as_u8(), Ordering::Relaxed);
        }
    }

    fn on_task_clear(&self, task: TaskId) {
        if let Some(slot) = self.slot(&task) {
            slot.state
                .store(Liveness::Running.as_u8(), Ordering::Relaxed);
        }
    }

    fn on_send_event(&self, _task: TaskId, _ev: UserEvent) {}

    fn on_cycle_stats(&self, obs: &CycleObservation) {
        if let Some(slot) = self.slot(&obs.task_id) {
            slot.period_ns.store(obs.period_ns, Ordering::Relaxed);
        }
    }
}

impl ExecutionMonitor for ExecutorBinding {
    fn post_execute(&self, task: TaskId, _at: Instant, took: Duration, ok: bool) {
        if let Some(slot) = self.slot(&task) {
            slot.record_took(took);
            if !ok {
                slot.errors.fetch_add(1, Ordering::Relaxed);
                slot.state.store(Liveness::Error.as_u8(), Ordering::Relaxed);
            }
        }
    }
}

// ── Provider read path (off the control path) ────────────────────────────────

impl Provider for ExecutorBinding {
    fn entities(&self) -> Vec<Entity> {
        let mut entities = Vec::with_capacity(self.task_ids.len() + 1);
        entities.push(self.executor_entity());
        for (task, slot) in self.task_ids.iter().zip(self.slots.iter()) {
            entities.push(Self::app_entity(task, slot));
        }
        entities
    }

    fn faults(&self, _entity_id: &str) -> Vec<FaultSummary> {
        Vec::new()
    }

    fn health(&self, entity_id: &str) -> Health {
        if entity_id == EXECUTOR_ID {
            return if self.executor_faulted.load(Ordering::Relaxed) {
                Health::Error
            } else {
                Health::Ok
            };
        }
        self.index
            .iter()
            .find(|(task, _)| Self::app_entity_id(task) == entity_id)
            .map_or(Health::Ok, |(_, &i)| {
                Liveness::from_u8(self.slots[i].state.load(Ordering::Relaxed)).health()
            })
    }

    fn snapshot(&self) -> ProviderSnapshot {
        let mut snapshot = ProviderSnapshot {
            entities: self.entities(),
            ..ProviderSnapshot::default()
        };
        for (task, slot) in self.task_ids.iter().zip(self.slots.iter()) {
            snapshot
                .data
                .insert(Self::app_entity_id(task), Self::app_data(slot));
        }
        snapshot
            .data
            .insert(EXECUTOR_ID.to_owned(), self.executor_data());
        snapshot
    }
}

impl ExecutorBinding {
    /// Build the readable `data` tree for the executor entity.
    fn executor_data(&self) -> Value {
        json!({
            "executor": {
                "up": self.executor_up.load(Ordering::Relaxed),
                "faulted": self.executor_faulted.load(Ordering::Relaxed),
                "task_count": self.task_ids.len(),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding() -> ExecutorBinding {
        ExecutorBinding::with_tasks(["ctrl", "io"])
    }

    #[test]
    fn unregistered_task_is_ignored() {
        let b = binding();
        b.on_app_start(TaskId::from("nope"), 1, None);
        // Only the executor entity plus the two registered apps are present.
        assert_eq!(b.entities().len(), 3);
        assert_eq!(b.health("app:nope"), Health::Ok);
    }

    #[test]
    fn liveness_tracks_lifecycle_hooks() {
        let b = binding();
        assert_eq!(b.health("app:ctrl"), Health::Ok);

        b.on_app_start(TaskId::from("ctrl"), 7, None);
        assert_eq!(b.health("app:ctrl"), Health::Ok);
        assert_eq!(
            Liveness::from_u8(b.slots[0].state.load(Ordering::Relaxed)),
            Liveness::Running
        );

        b.on_app_error(TaskId::from("ctrl"), &std::io::Error::other("boom"));
        assert_eq!(b.health("app:ctrl"), Health::Error);

        b.on_task_clear(TaskId::from("ctrl"));
        assert_eq!(b.health("app:ctrl"), Health::Ok);
    }

    #[test]
    fn executor_liveness_and_fault() {
        let b = binding();
        assert_eq!(b.health(EXECUTOR_ID), Health::Ok);
        b.on_executor_up();
        b.on_executor_fault(
            taktora_executor::ExecutorFaultReason::IterationBudgetExceeded {
                task_idx: 0,
                took_ms: 10,
                budget_ms: 5,
            },
        );
        assert_eq!(b.health(EXECUTOR_ID), Health::Error);
        b.on_executor_clear();
        assert_eq!(b.health(EXECUTOR_ID), Health::Ok);
    }

    #[test]
    fn timing_folds_into_data() {
        let b = binding();
        let now = Instant::now();
        b.post_execute(TaskId::from("ctrl"), now, Duration::from_micros(100), true);
        b.post_execute(TaskId::from("ctrl"), now, Duration::from_micros(200), true);
        b.on_cycle_stats(&CycleObservation {
            cycle_index: 0,
            task_id: TaskId::from("ctrl"),
            task_index: 0,
            faulted: false,
            period_ns: 1_000_000,
            pre_ns: 0,
            actual_period_ns: None,
            jitter_ns: None,
            lateness_ns: None,
            skipped_slots: 0,
            took_ns: None,
        });

        let snap = b.snapshot();
        let data = &snap.data["app:ctrl"]["timing"];
        assert_eq!(data["executions"], 2);
        assert_eq!(data["last_took_ns"], 200_000);
        assert_eq!(data["min_took_ns"], 100_000);
        assert_eq!(data["max_took_ns"], 200_000);
        assert_eq!(data["period_ns"], 1_000_000);
        assert_eq!(data["rate_hz"], 1000.0);
    }

    #[test]
    fn stop_does_not_clear_an_error() {
        let b = binding();
        b.on_app_error(TaskId::from("io"), &std::io::Error::other("x"));
        b.on_app_stop(TaskId::from("io"));
        assert_eq!(b.health("app:io"), Health::Error);
    }
}
