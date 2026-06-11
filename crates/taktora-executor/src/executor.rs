//! `Executor` and `ExecutorBuilder`. Run loop lives in Task 8.

// Fields consumed by the run loop (Task 8) and graph scheduler (Task 14).
#![allow(dead_code)]
// pub(crate) inside a private module — intentional, Task 8+ will use them.
#![allow(clippy::redundant_pub_crate)]

use crate::Channel;
use crate::clock::{MonotonicClock, SystemClock};
use crate::context::Stoppable;
use crate::error::ExecutorError;
use crate::fatal::{FatalDispatch, FatalHandler, FatalSite, guard_or_fatal, panic_payload_message};
use crate::fault::{
    ExecutorFaultAtomic, ExecutorFaultReason, ExecutorFaultState, FaultAtomic, FaultReason,
    FaultState, duration_to_ms_sat, instant_to_since_ms,
};
use crate::item::ExecutableItem;
use crate::monitor::{ExecutionMonitor, NoopMonitor};
use crate::observer::{NoopObserver, Observer};
use crate::payload::Payload;
use crate::pool::Pool;
use crate::stats::{CycleObservation, StatsSnapshot, TaskStatsEntry};
use crate::task_id::TaskId;
use crate::task_kind::TaskKind;
use crate::thread_attrs::ThreadAttributes;
use crate::trigger::{TriggerDecl, TriggerDeclarer};
use core::sync::atomic::AtomicU32;
use iceoryx2::node::Node;
use iceoryx2::port::listener::Listener as IxListener;
use iceoryx2::prelude::ipc;
use iceoryx2::prelude::*;
use iceoryx2::waitset::WaitSetRunResult;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use taktora_stats::ExecutorCycleStats;

/// Monotonically increasing counter so multiple executors in the same process
/// each get a unique stop-event service name.
static EXEC_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Executor histogram segment count (`S`) and exact-window length (`W`) for
/// per-task cycle stats. Fixed at compile time per `ADR_0060`.
pub(crate) type TaskCycleStats = ExecutorCycleStats<8, 256>;

/// A single wakeup's pending cycle record, stashed on the [`TaskEntry`] between
/// the pre-dispatch capture and the post-barrier fold. Bundling the pre-dispatch
/// timestamp with its `faulted` flag in one `Option` makes them impossible to
/// desync: a cycle is pending iff this is `Some`, and the `faulted` bit is then
/// always the one captured at the same wakeup (`REQ_0107`).
#[derive(Clone, Copy)]
pub(crate) struct CyclePending {
    /// Pre-dispatch timestamp for this wakeup (the cycle's `pre`), in
    /// telemetry-clock nanoseconds (see [`MonotonicClock`]).
    pub(crate) pre: u64,
    /// `true` when this wakeup's scan was fault-routed/skipped, so the
    /// post-barrier fold records it with `faulted=true`.
    pub(crate) faulted: bool,
}

/// One registered task entry.
pub(crate) struct TaskEntry {
    /// Task identifier.
    pub(crate) id: TaskId,
    /// The kind of work this entry holds (single item or chain).
    pub(crate) kind: TaskKind,
    /// Trigger declarations recorded at `add` time.
    pub(crate) decls: Vec<TriggerDecl>,
    /// Pre-allocated dispatch closure. Built once at `add` / `add_chain`
    /// time and re-invoked on every dispatch iteration via
    /// `Pool::submit_borrowed`, avoiding the per-iteration `Box::new(closure)`
    /// that `Pool::submit<F>` requires in threaded mode. Required for
    /// `REQ_0060` (zero-alloc steady-state dispatch). `None` for
    /// `TaskKind::Graph`, which dispatches its vertices via a separate
    /// path and is handled by `REQ_0062` / `REQ_0063` follow-on work.
    pub(crate) job: Option<Box<dyn FnMut() + Send + 'static>>,

    /// Per-task budget declared via `TriggerDeclarer::budget`. `None`
    /// means no per-task check; the executor-wide iteration budget
    /// still applies. `REQ_0070`.
    pub(crate) budget: Option<Duration>,

    /// Per-task fault state. Wait-free read on the dispatch hot path.
    /// Wrapped in `Arc` so dispatch closures built at `add` time can
    /// capture an owning handle into the same atomic the `TaskEntry`
    /// holds — `Arc::clone` is refcount-only, so this stays compatible
    /// with `REQ_0060` (no per-iteration allocation). `REQ_0070`.
    pub(crate) fault: Arc<FaultAtomic>,

    /// Monotonic per-task overrun counter. Increments on EVERY budget
    /// breach, including breaches while already `Faulted`. Never reset
    /// by clearing the fault. Shared with the dispatch closure via
    /// `Arc::clone`. `REQ_0102`.
    pub(crate) overrun_count: Arc<AtomicU64>,

    /// Pre-built dispatch closure for the fault-handler item. Mirrors
    /// `job`. `None` means no handler — the task is simply skipped
    /// during fault. `REQ_0072`.
    pub(crate) handler_job: Option<Box<dyn FnMut() + Send + 'static>>,

    /// Declared scan period for cyclic tasks (the `TriggerDecl::Interval`
    /// duration), or `None` for event-driven tasks. Cached at add time so the
    /// dispatch loop reads it without scanning `decls` per cycle. Gates cycle
    /// telemetry: only cyclic tasks participate (`REQ_0106`).
    pub(crate) scan_period: Option<Duration>,
    /// Last-cycle execute duration in ns, written by the dispatch closure on
    /// the pool worker and read by the `WaitSet` thread after `barrier()`.
    /// Shared via `Arc` exactly like `overrun_count`. Sentinel `u64::MAX` =
    /// "no sample this cycle" (the closure never ran — e.g. a faulted scan).
    pub(crate) last_took_ns: Arc<AtomicU64>,

    /// WaitSet-thread-only timestamp of this task's previous dispatch, for
    /// computing `actual_period` (`REQ_0101`). Not shared (no atomic) — only the
    /// single dispatch thread touches it. `None` before the first dispatch.
    /// Telemetry-clock nanoseconds (see [`MonotonicClock`]).
    pub(crate) last_dispatch: Option<u64>,

    /// WaitSet-thread-only lateness grid slot for this task (`REQ_0106`).
    /// Advances by exactly **one per scan attempt plus the dispatcher's
    /// skipped-slot signal** (`REQ_0840`) — never reconstructed from the
    /// noisy measured period, which over-counts on coalesced catch-up wakes
    /// (issue #46 / `ADR_0101`). Starts at `0` (the first recorded cycle is
    /// the task's own grid origin).
    pub(crate) grid_slot: u64,

    /// WaitSet-thread-only per-task lateness grid epoch (`REQ_0106`): the
    /// task's first recorded `pre` — including a faulted first scan, whose
    /// dispatch instant is real — back-dated by the dispatcher's `late_by`
    /// signal when one is present (`Grid` mode), so the grid anchors at the
    /// first dispatch's NOMINAL slot and a late process start is reported as
    /// real first-cycle lateness instead of becoming a permanent negative
    /// floor on every later on-grid cycle. Per-task (not executor-shared) so
    /// a task's start phase never reads as permanent lateness.
    pub(crate) grid_epoch: Option<u64>,

    /// WaitSet-thread-only dispatcher skip signal for the *current* wakeup
    /// (`REQ_0840`): written by the Grid dispatch pass before `dispatch_task`,
    /// consumed (`take`n) by `record_cycle_for`. Never written in `Legacy`
    /// mode or for event-driven tasks — stays `0`.
    pub(crate) pending_skipped: u32,

    /// WaitSet-thread-only dispatcher lateness signal for the *current*
    /// wakeup (`REQ_0106`): how far past its nominal grid slot this dispatch
    /// is, in scheduling-clock nanoseconds. Written by the Grid dispatch
    /// pass, consumed (`take`n) by `record_cycle_for`, which uses the FIRST
    /// one to anchor [`Self::grid_epoch`] on the nominal grid. `None` in
    /// `Legacy` mode and for event-driven tasks (no dispatcher grid exists:
    /// the epoch falls back to the first observed `pre`).
    pub(crate) pending_late: Option<u64>,

    /// WaitSet-thread-only stash of the *current* wakeup's pending cycle —
    /// the pre-dispatch timestamp plus its `faulted` flag — carried across
    /// `pool.barrier()` so the post-barrier record pass can fold this cycle's
    /// telemetry without re-reading the clock or allocating a fired-index list.
    /// `Some` between the pre-dispatch capture and the post-barrier
    /// `record_cycle_for` `take`; `None` otherwise. Bundling the timestamp and
    /// the fault flag in one `Option` keeps them from ever desyncing
    /// (`REQ_0107`). Only the single dispatch thread touches it (no atomic).
    pub(crate) pending_cycle: Option<CyclePending>,
}

/// Top-level executor. One per process is the typical case.
pub struct Executor {
    pub(crate) node: Node<ipc::Service>,
    pub(crate) pool: Arc<Pool>,
    pub(crate) tasks: Vec<TaskEntry>,
    /// One cycle-stats aggregator per registered task, index-aligned with
    /// `tasks`. Pushed at task-add time (before `run`), so no steady-state
    /// allocation (`REQ_0060`, `REQ_0104`). Updated single-writer on the
    /// `WaitSet` thread (Task 6).
    pub(crate) cycle_stats: Vec<TaskCycleStats>,
    /// Histogram sliding-window size in samples (`REQ_0100`).
    pub(crate) stats_window: u32,
    pub(crate) running: Arc<AtomicBool>,
    pub(crate) stoppable: Stoppable,
    pub(crate) next_id: AtomicU64,
    /// Listener for the internal stop event service. Held here so it outlives
    /// the `WaitSet` guard inside `dispatch_loop`. Created at `build()` time so
    /// any `Stoppable` clone (taken before or after `run()`) carries the waker.
    pub(crate) stop_listener: Arc<IxListener<ipc::Service>>,
    /// Lifecycle observer. Defaults to a no-op.
    pub(crate) observer: Arc<dyn Observer>,
    /// Execution monitor. Defaults to a no-op.
    pub(crate) monitor: Arc<dyn ExecutionMonitor>,
    /// Per-iteration error capture slot — allocated once at build time and
    /// reset to `None` at the top of each `dispatch_loop` iteration. Pool
    /// workers obtain a refcount-only `Arc::clone` of this slot, avoiding
    /// the per-iteration heap allocation that the previous design incurred.
    /// Required for `REQ_0060`.
    pub(crate) iter_err: Arc<std::sync::Mutex<Option<ExecutorError>>>,
    /// Executor-wide iteration budget from `ExecutorBuilder::iteration_budget`.
    /// `None` means no executor-wide check.
    pub(crate) iteration_budget: Option<Duration>,
    /// Executor-wide fault state. Wrapped in `Arc` so each dispatch
    /// closure can hold an owning handle without re-borrowing through
    /// `self`. `REQ_0071`.
    pub(crate) exec_fault: Arc<ExecutorFaultAtomic>,

    /// Index of the task whose `execute()` overran when the executor
    /// transitioned to `Faulted`. Read alongside `exec_fault`.
    pub(crate) exec_fault_task_idx: Arc<AtomicU32>,

    /// Budget that was breached when the executor transitioned to
    /// `Faulted`, in ms (saturated). Read alongside `exec_fault`.
    pub(crate) exec_fault_budget_ms: Arc<AtomicU32>,

    /// Executor start time, set on first dispatch. Used to compute
    /// `since_ms` for faults relative to `Executor::run` entry. Wrapped
    /// in `Arc` so dispatch closures share the same `OnceLock` with the
    /// executor — `get_or_init` is idempotent and wait-free.
    pub(crate) start_time: Arc<OnceLock<Instant>>,

    /// Fatal-dispatch handle. Called once on the fail-fast path from the
    /// executor-thread run-loop boundary; the pool holds a separate
    /// `Arc::clone` for its own worker / inline-submit boundaries.
    pub(crate) fatal_dispatch: Arc<FatalDispatch>,

    /// Telemetry time source (`REQ_0101`/`REQ_0105`/`REQ_0106`). Read on the
    /// worker (for `took`) and the `WaitSet` thread (for `pre`); defaults to
    /// [`SystemClock`]. A test can substitute a [`MockClock`] via
    /// [`ExecutorBuilder::clock`] for deterministic timing assertions. Affects
    /// only telemetry — never scheduling or fault behaviour.
    pub(crate) clock: Arc<dyn MonotonicClock>,

    /// Cyclic dispatch timing strategy (`REQ_0268` / `ADR_0100`). Read once at
    /// `dispatch_loop` entry and hoisted to a local, so steady-state cost is a
    /// single `Copy`-enum compare per cycle. Defaults to
    /// [`DispatchMode::Grid`](crate::DispatchMode).
    pub(crate) dispatch_mode: crate::DispatchMode,

    /// Scheduling time source for the absolute grid (`REQ_0268`). Distinct from
    /// [`Executor::clock`] (telemetry): a telemetry mock can never alter
    /// dispatch timing. Defaults to
    /// [`MonotonicCyclicClock`](crate::MonotonicCyclicClock).
    pub(crate) cyclic_clock: std::sync::Arc<dyn crate::CyclicClock>,
}

// SAFETY: `IxListener<ipc::Service>` is `!Send` for the same Rc-based
// `SingleThreaded` reason as `IxNotifier`. After construction, the only
// per-iteration call is `listener.try_wait_one()`, which does not mutate the
// Rc. `Executor` is never shared across threads (it requires `&mut self` for
// `run()`), so there is no aliased concurrent mutation.
#[allow(unsafe_code, clippy::non_send_fields_in_send_ty)]
unsafe impl Send for Executor {}

impl Executor {
    /// Start a new builder.
    #[must_use]
    pub fn builder() -> ExecutorBuilder {
        ExecutorBuilder::default()
    }

    /// Open or create a pub/sub channel bound to this executor's node.
    pub fn channel<T: Payload>(&mut self, name: &str) -> Result<Arc<Channel<T>>, ExecutorError> {
        Channel::open_or_create(&self.node, name)
    }

    /// Open or create a request/response service bound to this executor's node.
    pub fn service<Req, Resp>(
        &mut self,
        name: &str,
    ) -> Result<Arc<crate::Service<Req, Resp>>, ExecutorError>
    where
        Req: Payload,
        Resp: Payload,
    {
        crate::Service::open_or_create(&self.node, name)
    }

    /// Borrowed snapshot of every task's cycle aggregates (`REQ_0103` pull
    /// path). Relaxed reads; never blocks the dispatch writer.
    #[must_use]
    pub fn stats_snapshot(&self) -> StatsSnapshot {
        let per_task = self
            .tasks
            .iter()
            .zip(self.cycle_stats.iter())
            .map(|(t, s)| {
                let snap = s.snapshot();
                TaskStatsEntry {
                    task_id: t.id.clone(),
                    p50_ns: snap.p50_ns,
                    p95_ns: snap.p95_ns,
                    p99_ns: snap.p99_ns,
                    min_ns: snap.min_ns,
                    max_ns: snap.max_ns,
                    max_jitter_ns: snap.max_jitter_ns,
                    max_lateness_ns: snap.max_lateness_ns,
                    overrun_count: t.overrun_count.load(Ordering::Acquire),
                }
            })
            .collect();
        StatsSnapshot { per_task }
    }

    /// Add an item to the executor with an auto-generated id.
    pub fn add(&mut self, item: impl ExecutableItem) -> Result<TaskId, ExecutorError> {
        let id = TaskId::new(format!(
            "task-{}",
            self.next_id.fetch_add(1, Ordering::SeqCst)
        ));
        self.add_with_id(id, item)
    }

    /// Add an item with a user-supplied id.
    ///
    /// The item's [`ExecutableItem::task_id`] override takes precedence over
    /// the caller-supplied `id`, which itself takes precedence over the
    /// auto-generated id assigned by [`Executor::add`].
    pub fn add_with_id(
        &mut self,
        id: impl Into<TaskId>,
        mut item: impl ExecutableItem,
    ) -> Result<TaskId, ExecutorError> {
        let id_arg: TaskId = id.into();
        // The item's `task_id()` override wins over the user-supplied id.
        let id = item.task_id().map_or(id_arg, TaskId::new);
        let mut declarer = TriggerDeclarer::new_internal();
        item.declare_triggers(&mut declarer)?;
        let budget = declarer.budget;
        let decls = declarer.into_decls();

        // REQ_0268: reject ill-defined trigger shapes (cyclic+event, zero
        // period) before the task joins the table — the natural validation
        // point, where the decls are first available, for every DispatchMode.
        validate_decls(&id, &decls)?;

        let mut item_box: Box<dyn ExecutableItem> = Box::new(item);
        let app_id = item_box.app_id();
        let app_inst = item_box.app_instance_id();
        // SAFETY: the raw pointer points into the heap allocation of
        // `item_box`. `Box` keeps that allocation at a stable address even
        // when the `Box` itself is moved (e.g. when `self.tasks` grows),
        // so the pointer remains valid for the lifetime of the
        // `TaskEntry`. See SendItemPtr safety doc for the rest of the
        // discipline (barrier() pairs with worker access).
        #[allow(unsafe_code)]
        let item_ptr =
            SendItemPtr::new(std::ptr::from_mut::<dyn ExecutableItem>(item_box.as_mut()));

        // Allocate the per-task atomics now so the dispatch closure
        // and the `TaskEntry` share the same `Arc` storage. The task
        // will occupy `self.tasks.len()` after the push below — capture
        // that index up front for `task_idx_u32`. Bounded workspace, so
        // the `as u32` cast is sound; explicit allow keeps clippy quiet.
        let task_fault = Arc::new(FaultAtomic::new());
        let overrun_count = Arc::new(AtomicU64::new(0));
        let scan_period = scan_period_from_decls(&decls);
        let last_took_ns = Arc::new(AtomicU64::new(u64::MAX));
        #[allow(clippy::cast_possible_truncation)]
        let task_idx_u32 = self.tasks.len() as u32;
        let fault_ctx = FaultDispatchCtx {
            task_budget: budget,
            task_fault: Arc::clone(&task_fault),
            overrun_count: Arc::clone(&overrun_count),
            iteration_budget: self.iteration_budget,
            exec_fault: Arc::clone(&self.exec_fault),
            exec_fault_task_idx: Arc::clone(&self.exec_fault_task_idx),
            exec_fault_budget_ms: Arc::clone(&self.exec_fault_budget_ms),
            task_idx_u32,
            exec_start: Arc::clone(&self.start_time),
            observer: Arc::clone(&self.observer),
        };

        let job = build_single_job(
            id.clone(),
            self.stoppable.clone(),
            Arc::clone(&self.observer),
            Arc::clone(&self.monitor),
            Arc::clone(&self.iter_err),
            app_id,
            app_inst,
            item_ptr,
            fault_ctx,
            Arc::clone(&last_took_ns),
            Arc::clone(&self.clock),
        );

        self.tasks.push(TaskEntry {
            id: id.clone(),
            kind: TaskKind::Single(item_box),
            decls,
            job: Some(job),
            budget,
            fault: task_fault,
            overrun_count,
            handler_job: None,
            scan_period,
            last_took_ns: Arc::clone(&last_took_ns),
            last_dispatch: None,
            grid_slot: 0,
            grid_epoch: None,
            pending_skipped: 0,
            pending_late: None,
            pending_cycle: None,
        });
        self.cycle_stats
            .push(TaskCycleStats::new(self.stats_window));
        Ok(id)
    }

    /// Register an item plus a fault-handler item.
    ///
    /// The main item is registered through the canonical [`add`](Self::add)
    /// path. The handler's [`declare_triggers`](ExecutableItem::declare_triggers)
    /// is called (so handlers that internally rely on the declarer being
    /// invoked observe the call) but its returned trigger list is
    /// **ignored** — the handler dispatches on the main item's triggers
    /// while the task is in `Faulted` state and runs in place of the main
    /// item's `execute()`. The pre-built handler dispatch closure is
    /// stashed on the same task entry as the main item's `job`,
    /// satisfying `REQ_0072`.
    ///
    /// # Errors
    ///
    /// Propagates any error from registering the main item via `add`, or
    /// from the handler's `declare_triggers` call.
    ///
    /// # Panics
    ///
    /// Panics if the task entry just inserted by [`add`](Self::add) cannot
    /// be located in `self.tasks` — this is unreachable by construction
    /// and indicates a logic bug.
    pub fn add_with_fault_handler<I, H>(
        &mut self,
        main: I,
        handler: H,
    ) -> Result<TaskId, ExecutorError>
    where
        I: ExecutableItem,
        H: ExecutableItem,
    {
        let task_id = self.add(main)?;

        // Drain the handler's trigger declarations — they are ignored by
        // design (the handler runs on the main item's triggers).
        let mut handler_box: Box<dyn ExecutableItem> = Box::new(handler);
        let mut throwaway = TriggerDeclarer::new_internal();
        handler_box.declare_triggers(&mut throwaway)?;
        drop(throwaway);

        let app_id = handler_box.app_id();
        let app_inst = handler_box.app_instance_id();

        // Locate the task we just added so we can share its per-task
        // atomics with the handler's `FaultDispatchCtx`. The handler
        // runs on the same `TaskEntry`; per §4.6 invariant 5, a handler
        // breach increments `overrun_count` and keeps state `Faulted`
        // without re-firing the observer.
        let task_idx = self
            .tasks
            .iter()
            .position(|t| t.id == task_id)
            .expect("just added; must exist");
        let task = &self.tasks[task_idx];
        #[allow(clippy::cast_possible_truncation)]
        let task_idx_u32 = task_idx as u32;
        let handler_fault_ctx = FaultDispatchCtx {
            task_budget: task.budget,
            task_fault: Arc::clone(&task.fault),
            overrun_count: Arc::clone(&task.overrun_count),
            iteration_budget: self.iteration_budget,
            exec_fault: Arc::clone(&self.exec_fault),
            exec_fault_task_idx: Arc::clone(&self.exec_fault_task_idx),
            exec_fault_budget_ms: Arc::clone(&self.exec_fault_budget_ms),
            task_idx_u32,
            exec_start: Arc::clone(&self.start_time),
            observer: Arc::clone(&self.observer),
        };

        let handler_closure = build_handler_job(
            task_id.clone(),
            self.stoppable.clone(),
            Arc::clone(&self.observer),
            Arc::clone(&self.monitor),
            Arc::clone(&self.iter_err),
            app_id,
            app_inst,
            handler_box,
            handler_fault_ctx,
        );

        self.tasks[task_idx].handler_job = Some(handler_closure);

        Ok(task_id)
    }

    /// Clear a per-task fault. Returns the previous `FaultState`.
    /// Fires `Observer::on_task_clear` if the state changed from
    /// `Faulted` to `Running`. `REQ_0070`.
    ///
    /// # Errors
    ///
    /// * [`ExecutorError::TaskNotFound`] if `task` is unknown.
    /// * [`ExecutorError::TaskNotFaulted`] if `task` is already `Running`.
    pub fn clear_task_fault(&self, task: TaskId) -> Result<FaultState, ExecutorError> {
        let entry = self
            .tasks
            .iter()
            .find(|t| t.id == task)
            .ok_or_else(|| ExecutorError::TaskNotFound(task.clone()))?;
        let budget_ms = entry.budget.map_or(0_u32, crate::fault::duration_to_ms_sat);
        let prev = entry.fault.swap(FaultState::Running, budget_ms);
        match prev {
            FaultState::Running => Err(ExecutorError::TaskNotFaulted(task)),
            FaultState::Faulted { .. } => {
                self.observer.on_task_clear(task);
                Ok(prev)
            }
        }
    }

    /// Clear the executor-wide fault and cascade-clear every task whose
    /// state is `Faulted{ExecutorFaulted}`. Tasks whose state is
    /// `Faulted{BudgetExceeded}` are NOT cleared (their own contract
    /// breach is independent). Fires `Observer::on_executor_clear` and
    /// one `Observer::on_task_clear` per cascade-cleared task.
    /// `REQ_0071`.
    ///
    /// # Errors
    ///
    /// * [`ExecutorError::ExecutorNotFaulted`] if the executor is `Running`.
    pub fn clear_executor_fault(&self) -> Result<ExecutorFaultState, ExecutorError> {
        let task_idx = self.exec_fault_task_idx.load(Ordering::Acquire);
        let budget_ms = self.exec_fault_budget_ms.load(Ordering::Acquire);
        let prev = self
            .exec_fault
            .swap(ExecutorFaultState::Running, task_idx, budget_ms);
        match prev {
            ExecutorFaultState::Running => Err(ExecutorError::ExecutorNotFaulted),
            ExecutorFaultState::Faulted { .. } => {
                // Cascade-clear tasks whose reason is ExecutorFaulted.
                for entry in &self.tasks {
                    let task_budget_ms =
                        entry.budget.map_or(0_u32, crate::fault::duration_to_ms_sat);
                    if let FaultState::Faulted {
                        reason: FaultReason::ExecutorFaulted,
                        ..
                    } = entry.fault.load(task_budget_ms)
                    {
                        let _ = entry.fault.swap(FaultState::Running, task_budget_ms);
                        self.observer.on_task_clear(entry.id.clone());
                    }
                }
                self.observer.on_executor_clear();
                Ok(prev)
            }
        }
    }

    /// Return the per-task overrun counter — number of times the task's
    /// `execute()` exceeded its budget over the executor's lifetime.
    /// Monotonic; not reset by `clear_task_fault`. `REQ_0102`.
    ///
    /// # Errors
    ///
    /// * [`ExecutorError::TaskNotFound`] if `task` is unknown.
    pub fn overrun_count(&self, task: TaskId) -> Result<u64, ExecutorError> {
        self.tasks
            .iter()
            .find(|t| t.id == task)
            .map(|t| t.overrun_count.load(Ordering::Acquire))
            .ok_or_else(|| ExecutorError::TaskNotFound(task))
    }

    /// Return a snapshot of the per-task `FaultState`. `REQ_0073` (pull path).
    ///
    /// # Errors
    ///
    /// * [`ExecutorError::TaskNotFound`] if `task` is unknown.
    pub fn task_fault_state(&self, task: TaskId) -> Result<FaultState, ExecutorError> {
        self.tasks
            .iter()
            .find(|t| t.id == task)
            .map(|t| {
                let budget_ms = t.budget.map_or(0_u32, crate::fault::duration_to_ms_sat);
                t.fault.load(budget_ms)
            })
            .ok_or_else(|| ExecutorError::TaskNotFound(task))
    }

    /// Return a snapshot of the executor-wide `ExecutorFaultState`.
    /// `REQ_0073` (pull path).
    #[must_use]
    pub fn executor_fault_state(&self) -> ExecutorFaultState {
        let task_idx = self.exec_fault_task_idx.load(Ordering::Acquire);
        let budget_ms = self.exec_fault_budget_ms.load(Ordering::Acquire);
        self.exec_fault.load(task_idx, budget_ms)
    }

    /// Add a sequential chain of items. Only the head item's
    /// `declare_triggers` is consulted; non-head triggers are ignored with a
    /// tracing warn.
    pub fn add_chain<I, C>(&mut self, items: C) -> Result<TaskId, ExecutorError>
    where
        I: ExecutableItem,
        C: IntoIterator<Item = I>,
    {
        let id = TaskId::new(format!(
            "chain-{}",
            self.next_id.fetch_add(1, Ordering::SeqCst)
        ));
        let boxed: Vec<Box<dyn ExecutableItem>> = items
            .into_iter()
            .map(|i| Box::new(i) as Box<dyn ExecutableItem>)
            .collect();
        self.add_chain_with_id_boxed(id, boxed)
    }

    /// Like [`Executor::add_chain`] but with a user-supplied id.
    pub fn add_chain_with_id<I, C>(
        &mut self,
        id: impl Into<TaskId>,
        items: C,
    ) -> Result<TaskId, ExecutorError>
    where
        I: ExecutableItem,
        C: IntoIterator<Item = I>,
    {
        let boxed: Vec<Box<dyn ExecutableItem>> = items
            .into_iter()
            .map(|i| Box::new(i) as Box<dyn ExecutableItem>)
            .collect();
        self.add_chain_with_id_boxed(id.into(), boxed)
    }

    fn add_chain_with_id_boxed(
        &mut self,
        id: TaskId,
        mut items: Vec<Box<dyn ExecutableItem>>,
    ) -> Result<TaskId, ExecutorError> {
        if items.is_empty() {
            return Err(ExecutorError::Builder(
                "chain must contain at least one item".into(),
            ));
        }

        // Head item's `task_id()` override wins over the user-supplied id.
        let id = items[0].task_id().map_or(id, TaskId::new);

        // Head's triggers gate the chain.
        let mut head_declarer = TriggerDeclarer::new_internal();
        items[0].declare_triggers(&mut head_declarer)?;
        let decls = head_declarer.into_decls();

        // REQ_0268: same trigger-shape validation as the single-item path,
        // applied to the head item's decls (which gate the whole chain).
        validate_decls(&id, &decls)?;

        // Warn if non-head items declared triggers (those will be ignored).
        for (i, body) in items.iter_mut().enumerate().skip(1) {
            let mut spurious = TriggerDeclarer::new_internal();
            let _ = body.declare_triggers(&mut spurious);
            if !spurious.is_empty() {
                #[cfg(feature = "tracing")]
                tracing::warn!(
                    target: "taktora-executor",
                    task = %id,
                    position = i,
                    "non-head chain item declared triggers; they will be ignored"
                );
                #[cfg(not(feature = "tracing"))]
                {
                    let _ = i;
                }
            }
        }

        let mut items = items;
        // SAFETY: pointer into the chain's `items` Vec. The Vec lives
        // inside `TaskKind::Chain` inside `TaskEntry`. The Vec's buffer
        // is stable once `add_chain` returns — `self.tasks` may grow
        // (moving the `Vec<Box<...>>` header itself), but the Vec's
        // heap buffer is referenced via the header's data pointer and
        // is unaffected by header moves. We never resize the chain Vec
        // after this point. See SendChainPtr safety doc for the rest.
        #[allow(unsafe_code)]
        let chain_ptr = SendChainPtr::new(std::ptr::from_mut::<Vec<Box<dyn ExecutableItem>>>(
            &mut items,
        ));
        // NB: the pointer above is to the local `items` Vec on the
        // stack — it's invalid after the `push` below moves items into
        // the TaskEntry. We rederive a stable pointer after the push.
        // (See the rebuild step below.)
        let _ = chain_ptr;

        // Pre-allocate the per-task atomics so the chain's dispatch
        // closure can capture clones of the same `Arc`s the `TaskEntry`
        // holds. The chain occupies `self.tasks.len()` after the push.
        let task_fault = Arc::new(FaultAtomic::new());
        let overrun_count = Arc::new(AtomicU64::new(0));
        let scan_period = scan_period_from_decls(&decls);
        let last_took_ns = Arc::new(AtomicU64::new(u64::MAX));
        #[allow(clippy::cast_possible_truncation)]
        let task_idx_u32 = self.tasks.len() as u32;

        self.tasks.push(TaskEntry {
            id: id.clone(),
            kind: TaskKind::Chain(items),
            decls,
            job: None, // populated in the rebuild step below
            // TODO(post-Task-10): chain budgets carried separately; for now None.
            budget: None,
            fault: Arc::clone(&task_fault),
            overrun_count: Arc::clone(&overrun_count),
            handler_job: None,
            scan_period,
            last_took_ns: Arc::clone(&last_took_ns),
            last_dispatch: None,
            grid_slot: 0,
            grid_epoch: None,
            pending_skipped: 0,
            pending_late: None,
            pending_cycle: None,
        });
        self.cycle_stats
            .push(TaskCycleStats::new(self.stats_window));

        // After the push, the TaskEntry lives at a stable position in
        // `self.tasks` for the duration of this `add_chain_with_id_boxed`
        // call. Take a stable pointer to its chain Vec and build the
        // dispatch closure. If `self.tasks` later grows, the Vec header
        // inside the TaskEntry moves but the header's data pointer
        // (which addresses the chain's heap buffer) does not — and the
        // closure derefs that pointer per dispatch, so it re-reads the
        // current heap address each time. Sound under the same
        // discipline as `tasks_ptr` in dispatch_loop.
        let task_idx = self.tasks.len() - 1;
        let chain_vec_ptr: *mut Vec<Box<dyn ExecutableItem>> = match &mut self.tasks[task_idx].kind
        {
            TaskKind::Chain(v) => std::ptr::from_mut::<Vec<Box<dyn ExecutableItem>>>(v),
            // The push above used TaskKind::Chain, so this arm is
            // unreachable. Mark it explicitly to satisfy `match`.
            _ => unreachable!("just-pushed task is TaskKind::Chain"),
        };
        #[allow(unsafe_code)]
        let chain_ptr = SendChainPtr::new(chain_vec_ptr);
        let fault_ctx = FaultDispatchCtx {
            task_budget: None, // chain budgets are intentionally None for now
            task_fault,
            overrun_count,
            iteration_budget: self.iteration_budget,
            exec_fault: Arc::clone(&self.exec_fault),
            exec_fault_task_idx: Arc::clone(&self.exec_fault_task_idx),
            exec_fault_budget_ms: Arc::clone(&self.exec_fault_budget_ms),
            task_idx_u32,
            exec_start: Arc::clone(&self.start_time),
            observer: Arc::clone(&self.observer),
        };
        let job = build_chain_job(
            id.clone(),
            self.stoppable.clone(),
            Arc::clone(&self.observer),
            Arc::clone(&self.monitor),
            Arc::clone(&self.iter_err),
            chain_ptr,
            fault_ctx,
            Arc::clone(&last_took_ns),
            Arc::clone(&self.clock),
        );
        self.tasks[task_idx].job = Some(job);
        Ok(id)
    }

    /// Returns a [`Stoppable`] handle that is waker-aware from the moment the
    /// executor is built. Clone before calling `run()` — any clone taken at any
    /// time will wake the `WaitSet` when `stop()` is called.
    #[must_use]
    pub fn stoppable(&self) -> Stoppable {
        self.stoppable.clone()
    }

    /// Borrow the underlying iceoryx2 node (escape hatch for power users).
    pub const fn iceoryx_node(&self) -> &Node<ipc::Service> {
        &self.node
    }

    /// Begin building a graph. Call `.build()` on the returned builder to
    /// register the graph as a task.
    pub fn add_graph(&mut self) -> ExecutorGraphBuilder<'_> {
        ExecutorGraphBuilder {
            executor: self,
            builder: crate::graph::GraphBuilder::new(),
            custom_id: None,
        }
    }
}

/// Builder for [`Executor`].
pub struct ExecutorBuilder {
    worker_threads: Option<usize>,
    observer: Option<Arc<dyn Observer>>,
    monitor: Option<Arc<dyn ExecutionMonitor>>,
    worker_attrs: ThreadAttributes,
    /// Executor-wide iteration budget (`REQ_0071`). `None` means no
    /// executor-wide check.
    iteration_budget: Option<Duration>,
    /// User-supplied fatal handler. `None` → resolved to a no-op `Arc` in
    /// `build()`.
    fatal_handler: Option<FatalHandler>,
    /// Sliding-window size (samples) for cycle-stats aggregation
    /// (`REQ_0100`). `None` → resolved to `1024` in `build()`.
    stats_window: Option<u32>,
    /// Telemetry time source. `None` → resolved to [`SystemClock`] in
    /// `build()`. Override with a [`MockClock`](crate::MockClock) for
    /// deterministic timing tests.
    clock: Option<Arc<dyn MonotonicClock>>,
    /// Cyclic dispatch timing strategy (`REQ_0268`). Default
    /// [`DispatchMode::Grid`](crate::DispatchMode).
    dispatch_mode: crate::DispatchMode,
    /// Scheduling clock for the absolute grid. `None` → resolved to
    /// [`MonotonicCyclicClock`](crate::MonotonicCyclicClock) in `build()`.
    cyclic_clock: Option<std::sync::Arc<dyn crate::CyclicClock>>,
}

impl Default for ExecutorBuilder {
    fn default() -> Self {
        Self {
            worker_threads: None,
            observer: None,
            monitor: None,
            worker_attrs: ThreadAttributes::new(),
            iteration_budget: None,
            fatal_handler: None,
            stats_window: None,
            clock: None,
            dispatch_mode: crate::DispatchMode::default(),
            cyclic_clock: None,
        }
    }
}

impl ExecutorBuilder {
    /// Number of worker threads. `0` → inline (no pool). Default → physical
    /// cores.
    #[must_use]
    pub const fn worker_threads(mut self, n: usize) -> Self {
        self.worker_threads = Some(n);
        self
    }

    /// Attach a lifecycle observer. If not called, a no-op observer is used.
    #[must_use]
    pub fn observer(mut self, obs: Arc<dyn Observer>) -> Self {
        self.observer = Some(obs);
        self
    }

    /// Attach an execution monitor. If not called, a no-op monitor is used.
    #[must_use]
    pub fn monitor(mut self, mon: Arc<dyn ExecutionMonitor>) -> Self {
        self.monitor = Some(mon);
        self
    }

    /// Configure the executor-wide iteration budget. Any task whose
    /// `execute()` exceeds `dur` transitions the executor to `Faulted`
    /// (`REQ_0071`). Default: unset (no executor-wide check).
    #[must_use]
    pub const fn iteration_budget(mut self, dur: Duration) -> Self {
        self.iteration_budget = Some(dur);
        self
    }

    /// Sliding-window size (samples) for percentile / min-max / jitter /
    /// lateness aggregation (`REQ_0100`). Default `1024`.
    #[must_use]
    pub const fn stats_window(mut self, samples: u32) -> Self {
        self.stats_window = Some(samples);
        self
    }

    /// Substitute the telemetry time source. Defaults to [`SystemClock`].
    ///
    /// Pass a [`MockClock`](crate::MockClock) clone to drive `took` / jitter /
    /// lateness from scripted instants, making timing assertions exact and
    /// independent of the host scheduler. The clock affects telemetry only —
    /// scheduling, run-mode deadlines and fault detection always use the real
    /// monotonic clock.
    #[must_use]
    pub fn clock(mut self, clock: Arc<dyn MonotonicClock>) -> Self {
        self.clock = Some(clock);
        self
    }

    /// Select cyclic dispatch timing (default `DispatchMode::Grid`). `Legacy` is
    /// the pre-REQ_0268 `attach_interval` path, retained only until the Pi A/B.
    #[must_use]
    pub const fn dispatch_mode(mut self, mode: crate::DispatchMode) -> Self {
        self.dispatch_mode = mode;
        self
    }

    /// Override the scheduling clock (default `MonotonicCyclicClock`). Distinct
    /// from `clock` (telemetry) — see `CyclicClock`.
    #[must_use]
    pub fn cyclic_clock(mut self, clock: std::sync::Arc<dyn crate::CyclicClock>) -> Self {
        self.cyclic_clock = Some(clock);
        self
    }

    /// Set thread attributes (name prefix, CPU affinity, scheduling priority)
    /// for worker threads. Has no effect when `worker_threads` is `0` (inline
    /// mode). Requires the `thread_attrs` feature for non-default settings.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn worker_attrs(mut self, attrs: ThreadAttributes) -> Self {
        self.worker_attrs = attrs;
        self
    }

    /// Register a best-effort last-gasp handler invoked once on the fail-fast
    /// path immediately before `std::process::abort()`.
    ///
    /// **Contract**: runs over known-unsound executor state — MUST NOT touch
    /// executor internals; a panic inside the handler routes straight to
    /// `abort()`.
    ///
    /// The handler is expected to be time-bounded (the caller's responsibility);
    /// no runtime deadline is imposed.
    ///
    /// **Observer / monitor containment carve-out**: the panic containment
    /// described in the executor documentation covers only a user item's
    /// `execute()` call. Panics that originate in framework-invoked user
    /// callbacks that run *outside* that inner catch — such as
    /// [`Observer`](crate::Observer) methods (e.g. `on_app_error`,
    /// `on_task_fault`) and [`ExecutionMonitor`](crate::ExecutionMonitor)
    /// methods (e.g. `post_execute`) — escape to this fail-fast boundary and
    /// cause `abort()`. Those callbacks must therefore be treated as
    /// non-panicking by the implementor. See `REQ_0123`.
    ///
    /// If not called, a no-op handler is used and `abort()` is still reached
    /// after any unrecoverable fault.
    #[must_use]
    pub fn on_fatal(
        mut self,
        handler: impl Fn(&crate::FatalContext) + Send + Sync + 'static,
    ) -> Self {
        self.fatal_handler = Some(Arc::new(handler));
        self
    }

    /// Build the [`Executor`]. Creates a fresh iceoryx2 node and wires up the
    /// internal stop-event service so that any `Stoppable` clone (taken before
    /// or after `run()`) will wake the `WaitSet` when `stop()` is called.
    ///
    /// # Panics
    ///
    /// Panics if the internally-generated stop-event service name exceeds the
    /// iceoryx2 service name length limit (this cannot happen under normal use
    /// because the name is derived from the process id and a monotonic counter).
    #[allow(clippy::arc_with_non_send_sync)] // see SAFETY on `impl Send for Executor`
    #[track_caller]
    pub fn build(self) -> Result<Executor, ExecutorError> {
        let node = NodeBuilder::new()
            .create::<ipc::Service>()
            .map_err(ExecutorError::iceoryx2)?;

        let n_workers = self.worker_threads.unwrap_or_else(num_cpus::get_physical);

        // Resolve the fatal handler: use the user-supplied one or fall back to a no-op.
        let fatal_handler: FatalHandler = self
            .fatal_handler
            .unwrap_or_else(|| Arc::new(|_ctx: &crate::FatalContext| {}));
        let fatal_dispatch = Arc::new(FatalDispatch::new(fatal_handler));

        let pool = Arc::new(Pool::new(
            n_workers,
            self.worker_attrs,
            Arc::clone(&fatal_dispatch),
        )?);

        // Build the internal stop event service with a unique-per-process name
        // so multiple executors in the same process don't collide.
        let exec_seq = EXEC_COUNTER.fetch_add(1, Ordering::Relaxed);
        let stop_topic = format!(
            "taktora.exec.stop.{}.{exec_seq}.__taktora_event",
            std::process::id()
        );
        let stop_event = node
            .service_builder(&stop_topic.as_str().try_into().unwrap())
            .event()
            .open_or_create()
            .map_err(ExecutorError::iceoryx2)?;

        let stop_notifier = Arc::new(
            stop_event
                .notifier_builder()
                .create()
                .map_err(ExecutorError::iceoryx2)?,
        );

        // SAFETY: see module-level note; Arc<IxListener> is held here and only
        // accessed on the executor thread.
        let stop_listener = Arc::new(
            stop_event
                .listener_builder()
                .create()
                .map_err(ExecutorError::iceoryx2)?,
        );

        // Wire the notifier into the Stoppable so every clone is waker-aware
        // from the moment the executor is built.
        let stoppable = Stoppable::with_waker(stop_notifier);

        let observer: Arc<dyn Observer> = self.observer.unwrap_or_else(|| Arc::new(NoopObserver));

        let monitor: Arc<dyn ExecutionMonitor> =
            self.monitor.unwrap_or_else(|| Arc::new(NoopMonitor));

        let clock: Arc<dyn MonotonicClock> =
            self.clock.unwrap_or_else(|| Arc::new(SystemClock::new()));

        let cyclic_clock: std::sync::Arc<dyn crate::CyclicClock> = self
            .cyclic_clock
            .unwrap_or_else(|| std::sync::Arc::new(crate::MonotonicCyclicClock::new()));

        let exec = Executor {
            node,
            pool,
            tasks: Vec::new(),
            cycle_stats: Vec::new(),
            stats_window: self.stats_window.unwrap_or(1024),
            running: Arc::new(AtomicBool::new(false)),
            stoppable,
            next_id: AtomicU64::new(0),
            stop_listener,
            observer,
            monitor,
            iter_err: Arc::new(std::sync::Mutex::new(None)),
            iteration_budget: self.iteration_budget,
            exec_fault: Arc::new(ExecutorFaultAtomic::new()),
            exec_fault_task_idx: Arc::new(AtomicU32::new(0)),
            exec_fault_budget_ms: Arc::new(AtomicU32::new(0)),
            start_time: Arc::new(OnceLock::new()),
            fatal_dispatch,
            clock,
            dispatch_mode: self.dispatch_mode,
            cyclic_clock,
        };

        Ok(exec)
    }
}

// ── Run loop ──────────────────────────────────────────────────────────────────

impl Executor {
    /// Run the executor until [`Stoppable::stop`] is called or a task signals
    /// stop via [`crate::Context::stop_executor`].
    ///
    /// # Errors
    ///
    /// Returns the **first** [`ExecutorError`] surfaced during dispatch:
    ///
    /// * [`ExecutorError::Item`] if any item returns `Err` or panics.
    /// * [`ExecutorError::Iceoryx2`] if a `WaitSet` operation fails.
    /// * [`ExecutorError::AlreadyRunning`] if the executor is already running.
    ///
    /// If multiple items error in the same dispatch iteration, only the first
    /// is preserved; subsequent errors are discarded silently. To observe
    /// every error, attach an [`Observer`](crate::Observer) and read errors
    /// via [`Observer::on_app_error`](crate::Observer::on_app_error).
    pub fn run(&mut self) -> Result<(), ExecutorError> {
        self.run_inner(RunMode::Forever)
    }

    /// Run for at most `max` wall-clock duration, then return.
    ///
    /// # Errors
    ///
    /// Returns the **first** [`ExecutorError`] surfaced during dispatch:
    ///
    /// * [`ExecutorError::Item`] if any item returns `Err` or panics.
    /// * [`ExecutorError::Iceoryx2`] if a `WaitSet` operation fails.
    /// * [`ExecutorError::AlreadyRunning`] if the executor is already running.
    ///
    /// If multiple items error in the same dispatch iteration, only the first
    /// is preserved; subsequent errors are discarded silently. To observe
    /// every error, attach an [`Observer`](crate::Observer) and read errors
    /// via [`Observer::on_app_error`](crate::Observer::on_app_error).
    pub fn run_for(&mut self, max: Duration) -> Result<(), ExecutorError> {
        self.run_inner(RunMode::Until(Instant::now() + max))
    }

    /// Run until `n` full barrier-cycles (`WaitSet` wakeups) have completed.
    ///
    /// # Errors
    ///
    /// Returns the **first** [`ExecutorError`] surfaced during dispatch:
    ///
    /// * [`ExecutorError::Item`] if any item returns `Err` or panics.
    /// * [`ExecutorError::Iceoryx2`] if a `WaitSet` operation fails.
    /// * [`ExecutorError::AlreadyRunning`] if the executor is already running.
    ///
    /// If multiple items error in the same dispatch iteration, only the first
    /// is preserved; subsequent errors are discarded silently. To observe
    /// every error, attach an [`Observer`](crate::Observer) and read errors
    /// via [`Observer::on_app_error`](crate::Observer::on_app_error).
    pub fn run_n(&mut self, n: usize) -> Result<(), ExecutorError> {
        self.run_inner(RunMode::Iterations(n))
    }

    /// Run until `predicate()` returns true. Checked after each `WaitSet`
    /// wakeup.
    ///
    /// # Errors
    ///
    /// Returns the **first** [`ExecutorError`] surfaced during dispatch:
    ///
    /// * [`ExecutorError::Item`] if any item returns `Err` or panics.
    /// * [`ExecutorError::Iceoryx2`] if a `WaitSet` operation fails.
    /// * [`ExecutorError::AlreadyRunning`] if the executor is already running.
    ///
    /// If multiple items error in the same dispatch iteration, only the first
    /// is preserved; subsequent errors are discarded silently. To observe
    /// every error, attach an [`Observer`](crate::Observer) and read errors
    /// via [`Observer::on_app_error`](crate::Observer::on_app_error).
    pub fn run_until<F: FnMut() -> bool>(&mut self, mut predicate: F) -> Result<(), ExecutorError> {
        self.run_inner(RunMode::Predicate(&mut predicate))
    }
}

enum RunMode<'a> {
    Forever,
    Until(Instant),
    Iterations(usize),
    Predicate(&'a mut dyn FnMut() -> bool),
}

impl Executor {
    fn run_inner(&mut self, mut mode: RunMode<'_>) -> Result<(), ExecutorError> {
        // NOTE: Once `Stoppable::stop()` has been called, `self.stoppable.is_stopped()`
        // remains true permanently. Calling `run()` again after a stop will return
        // promptly without doing any meaningful work (it blocks until the first
        // trigger fires, then immediately exits the dispatch loop). Task 10's
        // Runner accommodates this by treating an Executor as one-shot: each
        // Runner owns the Executor and consumes it.
        if self.running.swap(true, Ordering::SeqCst) {
            return Err(ExecutorError::AlreadyRunning);
        }

        self.observer.on_executor_up();
        let result = self.dispatch_loop(&mut mode);
        match &result {
            Ok(()) => self.observer.on_executor_down(),
            Err(e) => self.observer.on_executor_error(e),
        }

        self.running.store(false, Ordering::SeqCst);
        result
    }

    #[deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    #[allow(
        unsafe_code,
        clippy::too_many_lines,
        clippy::ref_as_ptr,
        clippy::borrow_as_ptr
    )]
    fn dispatch_loop(&mut self, mode: &mut RunMode<'_>) -> Result<(), ExecutorError> {
        // Clear the per-wake transient tokens on every TaskEntry, once per
        // `run_*` call (O(task_count), alloc-free). `pending_cycle` doubles as
        // the per-phase dispatch dedup token (REQ_0854, #93), and its intended lifetime is
        // a single `dispatch_loop` invocation — but nothing enforced that
        // across a `run_*` re-entry, since `Executor` is `&mut self` and
        // `self.tasks` persists. A mid-wake fatal bail (`break Ok(())`) that
        // landed after a mark but before the post-barrier fold `take`d the
        // token would leave it `Some`, and the next `dispatch_loop`'s
        // `dispatch_task` guard would silently swallow that task's first
        // dispatch. Sweeping here makes the token lifetime one invocation by
        // construction (closes #102). Cross-cycle telemetry state
        // (`grid_epoch` / `grid_slot` / `last_dispatch`) is deliberately left
        // untouched — clearing it would corrupt the lateness grid.
        for task in &mut self.tasks {
            task.pending_cycle = None;
            task.pending_skipped = 0;
            task.pending_late = None;
        }

        // Tight timer slack for the dispatch thread (REQ_0274). SCHED_OTHER
        // threads inherit the kernel's 50 µs default, and `epoll_wait`
        // sleeps through `schedule_hrtimeout_range` WITH that slack: on the
        // Pi5 rig that cost 56 µs/cycle accumulated Legacy-dispatch drift
        // (5.5 µs/cycle at 1 µs slack — the residual is iceoryx2's
        // ms-rounded epoll timeout + wake latency). Real-time classes force
        // slack to 0, so this is a no-op under SCHED_FIFO and for the
        // Grid/timerfd path (fd readiness, not timeout, drives the wake);
        // it removes a 10× cyclic-precision regression for non-RT Legacy
        // deployments. Thread-local, never fails for valid args; an error
        // (exotic kernel) would only restore today's behavior, so the
        // return value is deliberately not checked.
        #[cfg(target_os = "linux")]
        // SAFETY: prctl(PR_SET_TIMERSLACK) only adjusts the calling
        // thread's timer slack; no memory is involved.
        unsafe {
            libc::prctl(libc::PR_SET_TIMERSLACK, 1_000u64, 0, 0, 0);
        }

        let waitset: WaitSet<ipc::Service> = WaitSetBuilder::new()
            .create()
            .map_err(ExecutorError::iceoryx2)?;

        // Keep Arc<RawListener> alive for at least as long as the WaitSet
        // guards — the guard borrows the listener via 'attachment lifetime.
        let mut listener_storage: Vec<Arc<crate::trigger::RawListener>> = Vec::new();
        // Guards must outlive the run loop.
        let mut guards: Vec<WaitSetGuard<'_, '_, ipc::Service>> = Vec::new();
        // Maps guard index → task index.
        let mut attachment_to_task: Vec<usize> = Vec::new();

        // Hoist to a local for the hot loop — one Copy-enum compare per cycle,
        // never a field re-read (REQ_0268).
        let dispatch_mode = self.dispatch_mode;

        // Cyclic tasks are dispatched by the master timer + GridTimer (REQ_0268),
        // not attached as individual WaitSet triggers. Cross-platform: only the
        // wake source differs (Task 3).
        let mut cyclic_task_indices: Vec<usize> = Vec::new();
        let mut cyclic_periods: Vec<u64> = Vec::new();
        let deadline_count = build_attachments(
            &waitset,
            &self.tasks,
            dispatch_mode,
            &mut listener_storage,
            &mut guards,
            &mut attachment_to_task,
            &mut cyclic_task_indices,
            &mut cyclic_periods,
        )?;
        // `deadline_count` is the number of attached `Deadline` guards; it sizes
        // the `AttachmentMap` id buckets so the lazy-learned Notification-form ids
        // (which a missed `Deadline` resolves to on its first miss) never realloc
        // in steady state. The map is built once, below, just after the guards are
        // final — `build_attachments` is the only populator of `guards` /
        // `attachment_to_task`, so nothing mutates them past this point.
        //
        // Borrow-check note: `attachment_map` aliases nothing in `self.tasks`; it
        // is a standalone local declared before the run `loop` and re-borrowed
        // `&mut` by each iteration's WaitSet callback. The callback's `resolve`
        // closure borrows `guards` / `attachment_to_task` *immutably* (via the
        // `DispatchPass` slice fields), while `map.resolve` holds `&mut map` — two
        // distinct objects, no aliasing, no raw pointer.
        let mut attachment_map = crate::attachment_map::AttachmentMap::build(
            &guards,
            &attachment_to_task,
            deadline_count,
        );
        // `cyclic_periods` is cloned, not moved, because Task 3 reads it again to
        // arm the single master timerfd; on non-Linux it is unused after this.
        let mut grid =
            crate::grid::GridTimer::new(self.cyclic_clock.now_nanos(), cyclic_periods.clone());
        let mut due_cyclic: Vec<(usize, u64, u64)> = Vec::new();

        // Master cyclic timer (REQ_0268, Linux). ONE timerfd armed at the base
        // period (gcd of cyclic periods) drives the absolute grid; GridTimer
        // decides which tasks are due each tick. Declared above its own guard so
        // it drops AFTER the guard (detach before close → no EBADF). Must be
        // declared here, after `build_attachments` has filled `cyclic_periods`.
        #[cfg(target_os = "linux")]
        let master_timer: Option<crate::timerfd::TimerFd> = {
            let base = crate::grid::base_period(&cyclic_periods);
            if base == 0 {
                None
            } else {
                Some(
                    crate::timerfd::TimerFd::new(std::time::Duration::from_nanos(base)).map_err(
                        |e| {
                            ExecutorError::DeclareTriggers(format!(
                                "failed to arm master timerfd: {e}"
                            ))
                        },
                    )?,
                )
            }
        };

        // Attach the master timer as a wake-only notification, held separately
        // (like the stop listener) so `process_attachment` never maps it to a
        // task. `_master_timer_guard` is declared immediately after `master_timer`
        // so on scope exit it drops FIRST — detaching the fd from the WaitSet's
        // epoll set — and `master_timer` drops SECOND, closing the fd. That
        // ordering is what prevents iceoryx2's `EPOLL_CTL_DEL` from hitting a
        // closed fd (EBADF). `master_timer`'s fd is referenced ONLY by this guard
        // (independent of `guards`/`listener_storage`, which own other fds), so
        // its drop position relative to those Vecs is immaterial.
        #[cfg(target_os = "linux")]
        #[allow(unsafe_code, clippy::ref_as_ptr, clippy::borrow_as_ptr)]
        let _master_timer_guard = match &master_timer {
            // SAFETY: `master_timer` is a stack local that outlives this guard
            // (declared above it); the cast erases the borrow lifetime to the
            // attachment lifetime, sound by the same discipline as the stop
            // listener. Dropped before `master_timer` closes the fd.
            Some(tf) => Some(
                waitset
                    .attach_notification(unsafe { &*(tf as *const crate::timerfd::TimerFd) })
                    .map_err(ExecutorError::iceoryx2)?,
            ),
            None => None,
        };

        // Attach the internal stop listener so the WaitSet wakes when
        // stop() is called. We hold `self.stop_listener` (Arc) in the Executor
        // struct which is valid for the lifetime of dispatch_loop. We use the
        // same raw-pointer-cast pattern as user listeners above.
        //
        // SAFETY: `self.stop_listener` is an Arc stored on `self`, which is
        // exclusively borrowed for the duration of `run_inner` (which calls
        // `dispatch_loop`). The listener is not freed while the guard is alive
        // because the Arc keeps it alive and `self` outlives this function.
        let stop_listener_ref: &IxListener<ipc::Service> =
            unsafe { &*(self.stop_listener.as_ref() as *const _) };
        let _stop_guard = waitset
            .attach_notification(stop_listener_ref)
            .map_err(ExecutorError::iceoryx2)?;

        let iterations_done = AtomicUsize::new(0);
        let stop_flag = self.stoppable.clone();

        loop {
            // Reset the pre-allocated per-iteration error slot (REQ_0060):
            // the slot is owned by `self.iter_err`, allocated once at build
            // time. Pool worker closures obtain a refcount-only clone of
            // the `Arc`; the slot itself is reused across iterations.
            #[allow(clippy::unwrap_used)]
            // fail-fast: poison unreachable — the lock is held only over an infallible Option insert/take, and any holder panic aborts the process before another thread observes it (ADR_0065)
            let mut iter_err_guard = self.iter_err.lock().unwrap();
            *iter_err_guard = None;
            drop(iter_err_guard);

            // SAFETY: we capture &mut self.tasks via a raw pointer because
            // wait_and_process expects FnMut and Rust can't see the closure
            // outlives `self`. The discipline that makes this sound:
            //   1. The closure body on the executor thread is the *only* code that
            //      reads `tasks_ptr`. The pool jobs it submits hold borrowed
            //      `*mut dyn ExecutableItem` slices into individual TaskEntries,
            //      not into the Vec itself, so they don't race with the Vec.
            //   2. `pool.barrier()` at the end of this callback ensures every
            //      submitted pool job has completed (and dropped its raw pointer)
            //      before the callback returns. The next iteration of the WaitSet
            //      loop is therefore the sole user of `tasks_ptr` again.
            //   3. The Vec is never resized inside this loop (no `push` / `remove`
            //      after dispatch starts), so the underlying buffer addresses are
            //      stable for the lifetime of `dispatch_loop`.
            let tasks_ptr = &mut self.tasks as *mut Vec<TaskEntry>;
            // Take the cycle_stats raw pointer before borrowing `observer`, so
            // the &mut borrow is released first — same discipline as tasks_ptr.
            let cycle_stats_ptr = &mut self.cycle_stats as *mut Vec<TaskCycleStats>;
            let observer = &self.observer;
            let pool = &self.pool;
            // Refcount-only clone of the pre-allocated error slot. Pool jobs
            // need a `'static` handle, and an `Arc::clone` does not allocate.
            // The Single/Chain paths use the closure baked into `task.job`,
            // which already captured stable Arc clones at `add`-time; the
            // Graph path uses closures pre-built by `prepare_dispatch`. Only
            // the error-aggregation logic on the WaitSet thread still needs
            // the slot here.
            let iter_err_inner = Arc::clone(&self.iter_err);
            // Raw pointer to the stop listener for draining inside the callback.
            // SAFETY: same as stop_listener_ref above — the Arc is alive for
            // the lifetime of dispatch_loop.
            let stop_listener_ptr = self.stop_listener.as_ref() as *const IxListener<ipc::Service>;
            // Raw pointer to the executor-wide fault state. Same safety
            // discipline as `tasks_ptr`: `Executor` is alive for the
            // duration of `dispatch_loop`; the WaitSet callback is the
            // only reader. REQ_0071. `self.exec_fault` is
            // `Arc<ExecutorFaultAtomic>` — we deref once to obtain a
            // pointer to the inner `ExecutorFaultAtomic`.
            let exec_fault_ptr = &*self.exec_fault as *const ExecutorFaultAtomic;
            // Raw pointer to the executor start time. Used by the lazy
            // cascade below to compute `since_ms` on task transitions
            // triggered by an executor-wide fault.
            let exec_start_ptr = &*self.start_time as *const OnceLock<Instant>;
            // Telemetry clock. Same lifetime/aliasing discipline as the
            // pointers above: the Executor outlives the dispatch loop and the
            // WaitSet callback is the sole reader.
            let clock = &self.clock;

            // Wrap the per-iteration dispatch body in the framework panic
            // boundary. A panic escaping here is *infrastructure* (the WaitSet
            // drive, pool submission/barrier, or dispatch wiring) — not a user
            // item panic, which is already caught and faulted inside
            // `run_item_catch_unwind`. On such a panic `guard_or_fatal` runs the
            // user fatal handler then aborts in production. Under a test
            // terminal it returns `None`, in which case we must NOT keep
            // iterating over possibly-corrupt executor state, so we break out.
            let Some(cb_result) =
                guard_or_fatal(&self.fatal_dispatch, FatalSite::ExecutorRunLoop, || {
                    // Bundle the per-iteration captures into a single context the
                    // WaitSet callback delegates to. Keeping the closure a thin
                    // adapter over `DispatchPass::process_attachment` keeps the
                    // dispatch logic in named, individually-measurable functions.
                    let mut pass = DispatchPass {
                        guards: &guards,
                        attachment_to_task: &attachment_to_task,
                        tasks_ptr,
                        cycle_stats_ptr,
                        observer,
                        exec_fault_ptr,
                        exec_start_ptr,
                        clock,
                        stop_listener_ptr,
                        pool,
                        iter_err: &iter_err_inner,
                    };

                    // Linux: block on fds — the master timerfd wakes us on the
                    // absolute grid. Non-Linux dev: bound the wait by the earliest
                    // pending grid target so the post-wait pass can dispatch.
                    #[cfg(target_os = "linux")]
                    let timeout = std::time::Duration::MAX;
                    #[cfg(not(target_os = "linux"))]
                    let timeout = match dispatch_mode {
                        crate::DispatchMode::Grid => {
                            grid.next_timeout(self.cyclic_clock.now_nanos())
                        }
                        crate::DispatchMode::Legacy => std::time::Duration::MAX,
                    };
                    waitset.wait_and_process_once_with_timeout(
                        |attachment_id: WaitSetAttachmentId<ipc::Service>| {
                            // `attachment_map` is a standalone local re-borrowed
                            // `&mut` each iteration; it aliases nothing `pass`
                            // already borrows (`pass` holds the `guards` /
                            // `attachment_to_task` slices immutably), so the two
                            // live side by side without conflict.
                            pass.process_attachment(&attachment_id, &mut attachment_map)
                        },
                        timeout,
                    )
                })
            else {
                // Only reachable under a test terminal (production aborts in
                // `fire`). Bail out of the run loop rather than continuing over
                // possibly-corrupt executor state.
                //
                // Unreachable in production: the production terminal aborts
                // before returning, so this branch exists solely so a
                // `#[cfg(test)]` recording terminal can unwind the loop.
                // Consequently, silently discarding any pending `iter_err`
                // here is immaterial to production behavior.
                break Ok(());
            };

            // Did the master timer tick this wake? Linux: drain it (clears epoll
            // readiness; >0 overruns means the absolute grid advanced). Non-Linux:
            // the self-computed timeout drove the wake, so always consult the grid
            // (take_due self-gates per task on `now >= next`). REQ_0268.
            #[cfg(target_os = "linux")]
            let ticked = master_timer.as_ref().is_some_and(|tf| tf.drain() > 0);
            #[cfg(not(target_os = "linux"))]
            let ticked = true;

            // Post-wait master-grid pass (Grid mode). `run_grid_cyclic_pass`
            // self-gates on `ticked` / stop-wake / mode / non-empty, then
            // dispatches EVERY due cyclic task atomically this tick (PLC
            // semantics). `cpass` is a side-effect-free bundle of borrows, so
            // building it unconditionally is free; the gate lives in the helper
            // to keep `dispatch_loop` within the complexity budget. REQ_0268.
            let cpass = DispatchPass {
                guards: &guards,
                attachment_to_task: &attachment_to_task,
                tasks_ptr,
                cycle_stats_ptr,
                observer,
                exec_fault_ptr,
                exec_start_ptr,
                clock,
                stop_listener_ptr,
                pool,
                iter_err: &iter_err_inner,
            };
            run_grid_cyclic_pass(
                cpass,
                ticked,
                dispatch_mode,
                &stop_flag,
                cb_result,
                &mut grid,
                self.cyclic_clock.now_nanos(),
                &cyclic_task_indices,
                &mut due_cyclic,
            );

            // Funnel the post-callback decision (interrupt / item error /
            // stop request / run-mode termination) through one helper that
            // yields a single control value, so the loop has exactly one exit.
            match self.after_callback(cb_result, mode, &iterations_done, &stop_flag) {
                IterOutcome::Continue => {}
                IterOutcome::Done => break Ok(()),
                IterOutcome::Failed(err) => break Err(err),
            }
        }
    }

    /// Evaluates the post-callback termination conditions for one dispatch
    /// iteration and reports whether the loop should continue, stop, or fail.
    ///
    /// Order of precedence matches the original inline checks: `WaitSet`
    /// errors, then SIGINT/SIGTERM, then a captured item error, then a stop
    /// request, then a bare-EINTR continue (`REQ_0269` — an interrupted wait
    /// is a spurious wake, not a termination), then the active [`RunMode`]
    /// limit.
    #[deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    fn after_callback(
        &self,
        cb_result: Result<WaitSetRunResult, iceoryx2::waitset::WaitSetRunError>,
        mode: &mut RunMode<'_>,
        iterations_done: &AtomicUsize,
        stop_flag: &Stoppable,
    ) -> IterOutcome {
        let cb_result = match cb_result.map_err(ExecutorError::iceoryx2) {
            Ok(r) => r,
            Err(e) => return IterOutcome::Failed(e),
        };

        // iceoryx2's WaitSet catches SIGINT/SIGTERM internally and reports
        // them as `TerminationRequest`; honor that here for a clean exit.
        if matches!(cb_result, WaitSetRunResult::TerminationRequest) {
            return IterOutcome::Done;
        }

        // `Interrupt` is a bare EINTR: ANY handled signal — job control
        // (SIGSTOP/SIGCONT), a debugger attach, a user-installed handler —
        // interrupts the reactor wait. That is a spurious wake, not a
        // termination request: a cyclic control runtime must keep running
        // (REQ_0269; found on the Pi5 rig, where `kill -STOP`/`-CONT` ended
        // `run_n(60000)` cleanly at ~5k cycles). Item errors and `stop()`
        // are still honored below; the wake is NOT counted as an iteration
        // (nothing was dispatched). A real SIGINT/SIGTERM still terminates:
        // iceoryx2 latches it and the NEXT wait call returns
        // `TerminationRequest` before blocking.
        let interrupted = matches!(cb_result, WaitSetRunResult::Interrupt);

        // Extract the error before dropping the MutexGuard — avoids holding the
        // lock across the return (clippy::significant_drop_in_scrutinee).
        #[allow(clippy::unwrap_used)]
        // fail-fast: poison unreachable — the lock is held only over an infallible Option insert/take, and any holder panic aborts the process before another thread observes it (ADR_0065)
        let maybe_err = self.iter_err.lock().unwrap().take();
        if let Some(err) = maybe_err {
            return IterOutcome::Failed(err);
        }
        if stop_flag.is_stopped() {
            return IterOutcome::Done;
        }

        // An interrupted wake is spurious (REQ_0269): it dispatched nothing,
        // so it neither counts as an iteration nor evaluates the run-mode
        // limit — it short-circuits to `Continue` and re-enters the wait.
        let reached_limit = !interrupted && {
            iterations_done.fetch_add(1, Ordering::SeqCst);
            match mode {
                RunMode::Forever => false,
                RunMode::Iterations(n) => iterations_done.load(Ordering::SeqCst) >= *n,
                RunMode::Until(deadline) => Instant::now() >= *deadline,
                RunMode::Predicate(p) => (p)(),
            }
        };
        if reached_limit {
            IterOutcome::Done
        } else {
            IterOutcome::Continue
        }
    }
}

/// Outcome of one `dispatch_loop` iteration's post-callback evaluation.
enum IterOutcome {
    /// Run another iteration.
    Continue,
    /// Terminate the loop successfully.
    Done,
    /// Terminate the loop with the given error.
    Failed(ExecutorError),
}

/// Post-wait absolute-grid pass (Grid mode only, `REQ_0268` / `ADR_0100`).
///
/// The `WaitSet` callback handles event/fd tasks; cyclic tasks are timed here
/// off the scheduling clock. `pass` mirrors the callback's `DispatchPass`
/// exactly — same borrows and raw pointers, same single-writer WaitSet-thread
/// discipline — and the callback is already dropped (its borrows freed) by the
/// time this runs. We poll `grid` for due cyclic slots, dispatch each due task,
/// and fold their telemetry through the SHARED [`DispatchPass::barrier_and_record`]
/// helper. This is a SEPARATE barrier phase from the callback's: each phase
/// barriers and folds only its own `pending_cycle` stashes, so cyclic tasks
/// record exactly once, identically to event tasks. We do NOT call
/// `record_cycle_for` directly here.
///
/// Self-gates and returns early (no dispatch, no record) unless this wake should
/// run the grid: the master timer ticked (`ticked`), we are in `Grid` mode, it is
/// not a stop wake, and there is at least one cyclic task with something due.
///
/// **Stop-wake suppression (`REQ_0268`)**: a `stop()` (or a SIGINT/SIGTERM
/// `cb_result`) must emit no spurious cyclic cycle — Legacy dispatches none on a
/// stop wake, so the grid path matches, or a `stop()` would emit one extra cycle
/// observation and desync the `FEAT_0038` `cycle_index` join key. Termination
/// itself is still decided by `after_callback`; this only suppresses the side
/// effects.
#[allow(clippy::too_many_arguments)]
fn run_grid_cyclic_pass(
    mut pass: DispatchPass<'_, '_, '_>,
    ticked: bool,
    dispatch_mode: crate::DispatchMode,
    stop_flag: &Stoppable,
    cb_result: Result<WaitSetRunResult, iceoryx2::waitset::WaitSetRunError>,
    grid: &mut crate::grid::GridTimer,
    now_nanos: u64,
    cyclic_task_indices: &[usize],
    due_cyclic: &mut Vec<(usize, u64, u64)>,
) {
    let stopping = stop_flag.is_stopped()
        || matches!(
            cb_result,
            Ok(WaitSetRunResult::Interrupt | WaitSetRunResult::TerminationRequest)
        );
    if !ticked
        || stopping
        || dispatch_mode != crate::DispatchMode::Grid
        || cyclic_task_indices.is_empty()
    {
        return;
    }
    grid.take_due(now_nanos, due_cyclic);
    if due_cyclic.is_empty() {
        return;
    }
    for (slot, skipped, late_by) in due_cyclic.iter() {
        pass.dispatch_cyclic(cyclic_task_indices[*slot], *skipped, *late_by);
    }
    pass.barrier_and_record();
}

/// Build every `WaitSet` attachment for the task table (`REQ_0268`). In `Grid`
/// mode, `TriggerDecl::Interval` cyclic tasks are only *collected* into
/// `cyclic_task_indices` / `cyclic_periods` — they are NOT attached as
/// individual `WaitSet` triggers. The master timer + `GridTimer` owns their
/// wakeups (cross-platform; wake-source wiring is done in the caller).
/// Every other decl (and every decl in `Legacy` mode, including `Interval`
/// via `attach_interval`) is attached normally. Extracted from `dispatch_loop`
/// to keep that function within the cyclomatic-complexity budget.
/// Today's linear resolution: returns the task index for the single guard the
/// fired id matches, or `IGNORE` if none. At most one guard can match (one fd
/// per attachment, unique tick indices), so first-match is exact.
///
/// Consumed by `process_attachment` via `AttachmentMap::resolve`; the
/// `O(log n)` map calls this as its `slow` fallback to seed the per-id cache.
/// Kept as a free fn (sibling of `attach_trigger_decl`) so both the map's
/// fallback and any direct caller share one definition.
fn linear_scan(
    guards: &[WaitSetGuard<'_, '_, ipc::Service>],
    attachment_to_task: &[usize],
    id: &WaitSetAttachmentId<ipc::Service>,
) -> usize {
    let mut found = crate::attachment_map::IGNORE;
    for i in 0..guards.len() {
        if id.has_event_from(&guards[i]) || id.has_missed_deadline(&guards[i]) {
            // Debug: keep scanning to assert no second guard matches.
            // Release: first match is exact (uniqueness invariant), stop early.
            //
            // `cfg!(debug_assertions)` is a RUNTIME bool, not a `#[cfg]`
            // attribute, so BOTH arms always compile — the Linux clippy run
            // still sees the early `return`. Deliberately NOT `#[cfg(...)]`,
            // which would gate a path the macOS clippy run skips.
            if cfg!(debug_assertions) {
                debug_assert_eq!(
                    found,
                    crate::attachment_map::IGNORE,
                    "id matched two guards"
                );
                found = attachment_to_task[i];
            } else {
                return attachment_to_task[i];
            }
        }
    }
    found
}

#[allow(clippy::too_many_arguments)]
fn build_attachments<'w>(
    waitset: &'w WaitSet<ipc::Service>,
    tasks: &[TaskEntry],
    dispatch_mode: crate::DispatchMode,
    listener_storage: &mut Vec<Arc<crate::trigger::RawListener>>,
    guards: &mut Vec<WaitSetGuard<'w, 'w, ipc::Service>>,
    attachment_to_task: &mut Vec<usize>,
    cyclic_task_indices: &mut Vec<usize>,
    cyclic_periods: &mut Vec<u64>,
) -> Result<usize, ExecutorError> {
    // Number of `Deadline` decls actually ATTACHED as guards (Grid-mode
    // `Interval` decls are diverted to the cyclic vecs and never counted).
    // Consumed by `AttachmentMap::build` (#94), which sizes its lazy-learned
    // deadline-id bucket from this exact count.
    let mut deadline_count = 0usize;
    for (task_idx, task) in tasks.iter().enumerate() {
        for decl in &task.decls {
            if dispatch_mode == crate::DispatchMode::Grid {
                if let TriggerDecl::Interval(d) = decl {
                    // Grid mode owns cyclic timing via the master timer + GridTimer;
                    // these decls are NOT attached as individual WaitSet triggers.
                    cyclic_task_indices.push(task_idx);
                    cyclic_periods.push(u64::try_from(d.as_nanos()).unwrap_or(u64::MAX));
                    continue;
                }
            }
            // Count `Deadline` only on the path where it is actually attached
            // (after the Grid-`Interval` `continue`), so the tally matches the
            // guards pushed below one-for-one.
            if matches!(decl, TriggerDecl::Deadline { .. }) {
                deadline_count += 1;
            }
            let guard = attach_trigger_decl(waitset, listener_storage, decl)?;
            guards.push(guard);
            attachment_to_task.push(task_idx);
        }
    }
    Ok(deadline_count)
}

/// Attaches a single [`TriggerDecl`] to `waitset`, returning the resulting
/// guard.
///
/// Listener-backed declarations (`Subscriber`, `Deadline`, `RawListener`)
/// clone the listener `Arc` into `listener_storage` to extend its lifetime to
/// the surrounding `dispatch_loop` scope; `Interval` attaches a bare timer.
///
/// # Safety
///
/// The returned guard borrows the listener via a raw-pointer cast that erases
/// its lifetime. Soundness relies on the caller keeping `listener_storage` (and
/// `waitset`) alive for at least as long as the guard, and dropping the guards
/// before `listener_storage` — exactly the discipline `dispatch_loop` follows.
#[allow(unsafe_code, clippy::ref_as_ptr, clippy::borrow_as_ptr)]
fn attach_trigger_decl<'w>(
    waitset: &'w WaitSet<ipc::Service>,
    listener_storage: &mut Vec<Arc<crate::trigger::RawListener>>,
    decl: &TriggerDecl,
) -> Result<WaitSetGuard<'w, 'w, ipc::Service>, ExecutorError> {
    // Clone the listener Arc and obtain a lifetime-erased reference. SAFETY:
    // both `listener_storage` and `waitset` are stack-local in `dispatch_loop`
    // and dropped together at its end; guards are dropped before
    // `listener_storage`.
    let mut listener_ref = |listener: &Arc<crate::trigger::RawListener>| {
        listener_storage.push(Arc::clone(listener));
        let l_ref = listener_storage.last().unwrap().as_ref();
        let l_ref: &crate::trigger::RawListener = unsafe { &*(l_ref as *const _) };
        l_ref
    };

    let guard = match decl {
        TriggerDecl::Subscriber { listener } | TriggerDecl::RawListener(listener) => {
            waitset.attach_notification(listener_ref(listener))
        }
        TriggerDecl::Interval(d) => waitset.attach_interval(*d),
        TriggerDecl::Deadline { listener, deadline } => {
            waitset.attach_deadline(listener_ref(listener), *deadline)
        }
    };
    guard.map_err(ExecutorError::iceoryx2)
}

/// Per-iteration dispatch context handed to the `WaitSet` callback.
///
/// `dispatch_loop` rebuilds one of these every iteration and the `WaitSet`
/// callback is a thin adapter over [`DispatchPass::process_attachment`]. All
/// fields are short-lived borrows / raw pointers into the `Executor` that owns
/// the surrounding `dispatch_loop`; their soundness is documented at each use
/// site in `dispatch_loop` (same single-threaded, barrier-bounded discipline).
struct DispatchPass<'a, 'g, 'w> {
    /// `WaitSet` guards, indexed in parallel with `attachment_to_task`.
    guards: &'a [WaitSetGuard<'g, 'w, ipc::Service>],
    /// Maps guard index to task index in `tasks_ptr`.
    attachment_to_task: &'a [usize],
    /// Raw pointer to `Executor::tasks`.
    tasks_ptr: *mut Vec<TaskEntry>,
    /// Raw pointer to `Executor::cycle_stats` (index-aligned with `tasks`).
    cycle_stats_ptr: *mut Vec<TaskCycleStats>,
    /// Borrow of the executor's observer for the `on_cycle_stats` push.
    observer: &'a Arc<dyn Observer>,
    /// Raw pointer to `Executor::exec_fault` inner state.
    exec_fault_ptr: *const ExecutorFaultAtomic,
    /// Raw pointer to `Executor::start_time`.
    exec_start_ptr: *const OnceLock<Instant>,
    /// Borrow of the executor's telemetry clock, read for each cycle's `pre`.
    clock: &'a Arc<dyn MonotonicClock>,
    /// Raw pointer to the internal stop listener.
    stop_listener_ptr: *const IxListener<ipc::Service>,
    /// Borrow of the executor thread pool.
    pool: &'a Pool,
    /// Refcount-only handle to the per-iteration error slot.
    iter_err: &'a Arc<std::sync::Mutex<Option<ExecutorError>>>,
}

impl DispatchPass<'_, '_, '_> {
    /// Dispatches a single task by index for one wakeup: takes the `&mut`
    /// borrow into the task table, applies the pre-dispatch fault gate, stashes
    /// this cycle's `pending_cycle` timestamp for the post-barrier telemetry
    /// fold, and submits the task's work to the pool.
    ///
    /// Shared by the `WaitSet` callback (`process_attachment`) and — per
    /// `REQ_0268` / `ADR_0100` — the forthcoming post-wait absolute-grid
    /// dispatch pass, so the per-task barrier/telemetry contract is identical
    /// across both call paths.
    /// Grid-mode cyclic dispatch: stashes the dispatcher's skipped-slot
    /// signal (`REQ_0840`) on the task, then runs the shared dispatch path —
    /// the post-barrier `record_cycle_for` `take`s it and folds
    /// `grid_slot += 1 + skipped` (`REQ_0106` / `ADR_0101`). Only this
    /// discrete count crosses the scheduler→telemetry boundary; timestamps
    /// stay on the telemetry clock (`REQ_0268`).
    #[allow(unsafe_code)]
    fn dispatch_cyclic(&mut self, task_idx: usize, skipped: u64, late_by: u64) {
        // SAFETY: same single-writer WaitSet-thread discipline as
        // dispatch_task; the pointer is valid for the duration of this call.
        let task = unsafe { &mut (&mut *self.tasks_ptr)[task_idx] };
        // Post-validate_decls one-interval-per-task (#93), this fires at most
        // once per task per grid pass — so these pending_skipped/pending_late
        // writes cannot be clobbered by a same-phase re-entry, and the
        // dispatch_task `pending_cycle` guard is the structural backstop if that
        // invariant ever changes (e.g. the follow-up batched-barrier slice).
        task.pending_skipped = u32::try_from(skipped).unwrap_or(u32::MAX);
        task.pending_late = Some(late_by);
        self.dispatch_task(task_idx);
    }

    #[deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    #[allow(unsafe_code)]
    fn dispatch_task(&mut self, task_idx: usize) {
        // SAFETY: we are the only thread that may touch the task table
        // during the callback. wait_and_process_once is single-threaded
        // and dispatch_loop holds &mut self. The pointer is valid for the
        // duration of this call.
        let task = unsafe { &mut (&mut *self.tasks_ptr)[task_idx] };

        // Per-phase dispatch dedup (REQ_0854, #93). `pending_cycle` is set at
        // dispatch and `take`n only by `barrier_and_record`. Still `Some` ⇒ this
        // task was already dispatched this wake-phase with no intervening
        // barrier; re-submitting the borrowed job (main item OR fault handler)
        // would alias one `*mut dyn FnMut` across two pool workers. Skip — the
        // first run's listener `take()` loop drains all pending input. This is
        // the structural backstop that makes the grid path (one barrier after
        // the whole due-loop) and the future batched-barrier slice sound by
        // construction.
        if task.pending_cycle.is_some() {
            return;
        }

        // Pre-dispatch fault check (REQ_0070, REQ_0071, REQ_0072). When it
        // routes to a (possible) handler, normal dispatch is skipped.
        if self.handle_fault_routing(task) {
            // REQ_0107: a faulted/fault-routed scan STILL advances
            // cycle_index and emits on_cycle_stats, or the executor's count
            // desyncs from the connector's join key (FEAT_0038). took/jitter
            // are None (poison-safe); the index always moves. Allocation-free:
            // a CyclePending { Instant, bool } written onto the TaskEntry,
            // no heap.
            //
            // Set unconditionally (REQ_0854, #93): `pending_cycle` doubles as
            // the per-phase dedup token for ALL task kinds, so it must cover the
            // borrowed fault-handler submit too, not just cyclic tasks. Setting
            // it for an event task is telemetry-neutral — `record_cycle_for`
            // opens with `let Some(period) = task.scan_period else { return }`,
            // before any state mutation, so an event task's token produces zero
            // telemetry side effects (and REQ_0107's cyclic-faulted-scan cycle
            // advance is unaffected).
            task.pending_cycle = Some(CyclePending {
                pre: self.clock.now_nanos(),
                faulted: true,
            });
            return;
        }

        // Stash the pre-dispatch instant so the post-barrier record pass
        // can fold this cycle's telemetry. Allocation-free: the timestamp
        // lives on the TaskEntry, not in a per-wakeup Vec. `take`n in the
        // post-barrier loop below — guarantees exactly-once even if two
        // guards map to the same task. `faulted: false`: a task that faulted
        // last wakeup and recovered this one records the normal path (the
        // whole CyclePending is overwritten, so the flag can't be stale).
        task.pending_cycle = Some(CyclePending {
            pre: self.clock.now_nanos(),
            faulted: false,
        });

        self.submit_task_job(task);
    }

    /// Handles a single `WaitSet` wakeup: drains stop notifications, then
    /// resolves the fired attachment id via `map` (an `O(log n)` lookup that
    /// falls back to `linear_scan` once per never-before-seen id, caching the
    /// result) and dispatches the single matching task. Always returns
    /// [`CallbackProgression::Continue`]; termination is decided by the
    /// `stop_flag` check in `dispatch_loop` after the callback returns.
    ///
    /// Behaviour is identical to the prior linear guard sweep: at most one guard
    /// matches a given fired id (uniqueness invariant), so resolving-then-
    /// dispatching the one match is equivalent to looping and dispatching every
    /// match. Wake-only attachments (stop listener, master timer) resolve to
    /// [`crate::attachment_map::IGNORE`] and dispatch nothing.
    #[deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    #[allow(unsafe_code)]
    fn process_attachment(
        &mut self,
        attachment_id: &WaitSetAttachmentId<ipc::Service>,
        map: &mut crate::attachment_map::AttachmentMap,
    ) -> CallbackProgression {
        // Drain stop notifications first (no dispatch — the stop_flag check
        // after the callback returns handles termination).
        // SAFETY: stop_listener_ptr is valid for the duration of the call;
        // the Arc in self.stop_listener keeps it alive.
        let stop_l = unsafe { &*self.stop_listener_ptr };
        while let Ok(Some(_)) = stop_l.try_wait_one() {}

        // Resolve the fired id to its task index. `map` caches per-id; the
        // `slow` fallback is `linear_scan` over the same guard/task slices the
        // old loop walked. Capture the two slice fields locally so the closure
        // borrows just them (a shared borrow) rather than `&self` — `map` is a
        // separate object held `&mut`, so the borrows do not alias.
        let guards = self.guards;
        let attachment_to_task = self.attachment_to_task;
        let task_idx = map.resolve(attachment_id, |id| {
            linear_scan(guards, attachment_to_task, id)
        });
        if task_idx != crate::attachment_map::IGNORE {
            self.dispatch_task(task_idx);
        }

        self.barrier_and_record();

        CallbackProgression::Continue
    }

    /// Barrier all submitted pool jobs for this dispatch phase, then fold each
    /// task's stashed `pending_cycle` into recorded cycle telemetry. Shared by
    /// the `WaitSet` callback (event/fd tasks) and the post-wait grid pass
    /// (cyclic tasks, `REQ_0268`). Keyed on `pending_cycle` so it records
    /// exactly the tasks dispatched this phase, exactly once.
    #[deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    #[allow(unsafe_code)]
    fn barrier_and_record(&mut self) {
        // Wait for all submitted jobs to finish before leaving the callback
        // scope (validates item_ptr safety contract). The barrier also makes
        // every worker's `last_took_ns` Release-store visible to the record
        // pass below.
        self.pool.barrier();

        // Post-barrier telemetry fold. The source of truth for "this task was
        // dispatched this wakeup and owes a record" is `pending_cycle`,
        // set in the dispatch loop above — not the guard fired-status. Keying
        // solely on the stash (rather than re-querying `has_event_from`)
        // removes any dependency on the fired-status query being stable across
        // a second scan, so a dispatched cycle can never be silently
        // under-recorded (which would lag `cycle_index` — the desync FEAT_0038
        // must avoid). `take` clears the stash, guaranteeing exactly-once.
        // Allocation-free: iterate task indices in place.
        // SAFETY: same single-writer WaitSet-thread discipline as the dispatch
        // loop above; barrier-bounded, no in-flight pool job aliases `tasks`.
        let task_count = unsafe { (*self.tasks_ptr).len() };
        for task_idx in 0..task_count {
            // SAFETY: single-writer WaitSet thread; borrow released before
            // the record_cycle_for call (which re-derefs tasks_ptr).
            let pending = unsafe { (&mut *self.tasks_ptr)[task_idx].pending_cycle.take() };
            if let Some(CyclePending { pre, faulted }) = pending {
                self.record_cycle_for(task_idx, faulted, pre);
            }
        }
    }

    /// Fold one scan cycle's telemetry and push it to the observer. Called
    /// once per fired CYCLIC attachment per wakeup. `faulted = true` (Task 10)
    /// means the scan was skipped/errored: `took`/`jitter`/`lateness` are
    /// unmeasured. Event-driven tasks (no `scan_period`) are skipped entirely
    /// (`REQ_0106`).
    #[allow(unsafe_code)]
    fn record_cycle_for(&mut self, task_idx: usize, faulted: bool, pre_ns: u64) {
        // SAFETY: single-writer WaitSet thread; same discipline as tasks_ptr.
        let task = unsafe { &mut (&mut *self.tasks_ptr)[task_idx] };
        let Some(period) = task.scan_period else {
            return; // event-driven: no cycle telemetry
        };
        let period_ns = u64::try_from(period.as_nanos()).unwrap_or(u64::MAX);

        // Release/Acquire pairing with the worker store (M2): `swap` acquires
        // the worker's Release-store and resets the sentinel atomically.
        let took_raw = task.last_took_ns.swap(u64::MAX, Ordering::AcqRel);
        let took = if faulted || took_raw == u64::MAX {
            None
        } else {
            Some(took_raw)
        };

        // actual_period + jitter vs the previous dispatch (REQ_0101). Always
        // advance `last_dispatch` (even on a faulted attempt) so the next
        // cycle's period is measured from this wakeup. `actual_period` is
        // `None` on the very first cycle (no previous timestamp); jitter is
        // additionally suppressed on a faulted scan (poison-safe: REQ_0107).
        let actual_period = task
            .last_dispatch
            .replace(pre_ns)
            .map(|prev| pre_ns.saturating_sub(prev));
        let jitter = if faulted {
            None
        } else {
            actual_period.map(|ap| ap.abs_diff(period_ns))
        };

        // Per-task lateness grid (REQ_0106 / ADR_0101): the task's first
        // record anchors the grid at slot 0 (a faulted first scan anchors
        // too, its dispatch instant is real); every later record advances by
        // exactly one slot plus the dispatcher's skipped-slot signal
        // (REQ_0840). Never reconstructed from the measured period, which
        // over-counts on coalesced catch-up wakes and fabricates negative
        // lateness (issue #46). The anchor is the first dispatch's NOMINAL
        // slot — `pre` back-dated by the dispatcher's `late_by` signal — so
        // a late process start is reported as real first-cycle lateness
        // instead of becoming a permanent negative floor on later on-grid
        // cycles. `late_by` is a scheduling-clock difference applied to a
        // telemetry-clock instant: domain-safe, both are monotonic ns.
        // Without a dispatcher signal (Legacy mode, event-driven tasks) the
        // anchor stays the first observed `pre`.
        let skipped = core::mem::take(&mut task.pending_skipped);
        let late_by = task.pending_late.take();
        let first = task.grid_epoch.is_none();
        let grid_epoch = *task
            .grid_epoch
            .get_or_insert_with(|| pre_ns.saturating_sub(late_by.unwrap_or(0)));
        if !first {
            task.grid_slot = task
                .grid_slot
                .saturating_add(1)
                .saturating_add(u64::from(skipped));
        }
        let grid_slot = task.grid_slot;

        // SAFETY: cycle_stats is index-aligned with tasks; single-writer.
        let stats = unsafe { &mut (&mut *self.cycle_stats_ptr)[task_idx] };

        // Deadline lateness (REQ_0106): signed offset of the actual start
        // (`pre_ns`) from its nominal grid point `grid_epoch + grid_slot *
        // period`. Positive => started late; negative => early. Captures
        // steady drift (jitter is blind to a constant offset; lateness is
        // not). A dispatcher skip re-anchors via `skipped` above; absent a
        // signal (e.g. `Legacy` mode, which never signals), a whole missed
        // period honestly remains visible as a persistent offset rather than
        // being absorbed.
        let lateness = if period_ns > 0 && !faulted {
            let elapsed_ns = i64::try_from(pre_ns.saturating_sub(grid_epoch)).unwrap_or(i64::MAX);
            let expected_ns =
                i64::try_from(u128::from(grid_slot) * u128::from(period_ns)).unwrap_or(i64::MAX);
            Some(elapsed_ns.saturating_sub(expected_ns))
        } else {
            None
        };

        let cycle_index = stats.record_cycle(took, jitter, lateness);

        let obs = CycleObservation {
            cycle_index,
            task_id: task.id.clone(),
            task_index: u32::try_from(task_idx).unwrap_or(u32::MAX),
            faulted,
            period_ns,
            pre_ns,
            actual_period_ns: actual_period,
            jitter_ns: jitter,
            lateness_ns: lateness,
            skipped_slots: skipped,
            took_ns: took,
        };
        self.observer.on_cycle_stats(&obs);
    }

    /// Applies the pre-dispatch fault gate for `Single`/`Chain` tasks.
    ///
    /// Returns `true` when the task is routed to its fault handler (or
    /// silently skipped because no handler is registered) and normal dispatch
    /// must therefore be skipped. Returns `false` when normal dispatch should
    /// proceed. `Graph` tasks always return `false` — they use their own
    /// per-vertex scheduling and are out of scope for `FEAT_0018`.
    #[allow(unsafe_code, clippy::ref_as_ptr, clippy::borrow_as_ptr)]
    fn handle_fault_routing(&self, task: &mut TaskEntry) -> bool {
        if !matches!(task.kind, TaskKind::Single(_) | TaskKind::Chain(_)) {
            return false;
        }

        // SAFETY: exec_fault_ptr derefs into the Executor that owns the
        // surrounding dispatch_loop — alive for this call's lifetime.
        let exec_faulted = matches!(
            unsafe { &*self.exec_fault_ptr }.load(0, 0),
            ExecutorFaultState::Faulted { .. }
        );
        let task_budget_ms = task.budget.map_or(0_u32, duration_to_ms_sat);
        let task_state = task.fault.load(task_budget_ms);

        // Lazy cascade: if executor is `Faulted` and task is still `Running`,
        // silently transition the task to `Faulted{ExecutorFaulted}`. No
        // `on_task_fault` — the Observer already heard about the executor-wide
        // fault via `on_executor_fault` (cascade-noise invariant, FEAT_0018
        // §4.6).
        let task_faulted = if exec_faulted && matches!(task_state, FaultState::Running) {
            // SAFETY: exec_start_ptr derefs into the same Executor owning the
            // dispatch_loop. The OnceLock is wait-free.
            let exec_start = *unsafe { &*self.exec_start_ptr }.get_or_init(std::time::Instant::now);
            let since_ms = instant_to_since_ms(std::time::Instant::now(), exec_start);
            let _ = task.fault.swap(
                FaultState::Faulted {
                    reason: FaultReason::ExecutorFaulted,
                    since_ms,
                },
                task_budget_ms,
            );
            true
        } else {
            matches!(task_state, FaultState::Faulted { .. })
        };

        if !(exec_faulted || task_faulted) {
            return false;
        }

        // If a handler is registered, dispatch it. Otherwise, skip dispatch
        // entirely this wakeup.
        if let Some(handler_box) = task.handler_job.as_deref_mut() {
            let job_ptr: *mut (dyn FnMut() + Send) = handler_box as *mut (dyn FnMut() + Send);
            // SAFETY: same as the main-job dispatch below — handler_job is
            // owned by the TaskEntry; pool.barrier() awaits its completion
            // before the next callback.
            unsafe {
                self.pool
                    .submit_borrowed(crate::pool::BorrowedJob::new(job_ptr));
            }
        }
        true
    }

    /// Dispatches `task`'s normal (non-fault) work for one wakeup.
    ///
    /// `Single`/`Chain` tasks submit their pre-built job to the pool;
    /// `Graph` tasks drive one pass and capture the first item error into the
    /// per-iteration error slot.
    #[deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    #[allow(unsafe_code, clippy::ref_as_ptr, clippy::borrow_as_ptr)]
    fn submit_task_job(&self, task: &mut TaskEntry) {
        match &mut task.kind {
            TaskKind::Single(_) | TaskKind::Chain(_) => {
                // The dispatch closure was pre-allocated at task-add time and
                // stashed on `task.job`. Submit it via `submit_borrowed` — no
                // per-iteration Box allocation. Required by REQ_0060.
                #[allow(clippy::expect_used)]
                // fail-fast: Single/Chain task.job is always Some — set at add time in build_single_job/build_chain_job and never cleared
                let job_box = task
                    .job
                    .as_deref_mut()
                    .expect("Single/Chain tasks carry a pre-built job");
                let job_ptr: *mut (dyn FnMut() + Send) = job_box as *mut (dyn FnMut() + Send);
                // SAFETY: the closure lives in `task.job`, owned by
                // `self.tasks[task_idx]`; `tasks_ptr` is sound for the
                // duration of this callback. `pool.barrier()` in
                // `process_attachment` finishes the closure invocation before
                // the next iteration's callback. The WaitSet thread does not
                // touch the closure between this submit and that barrier.
                unsafe {
                    self.pool
                        .submit_borrowed(crate::pool::BorrowedJob::new(job_ptr));
                }
            }
            TaskKind::Graph(graph) => {
                // Outer driver runs on the WaitSet thread; vertices run on the
                // pool. The graph holds its own pre-built per-vertex closures
                // and SPSC ready ring (REQ_0060), so dispatch is
                // allocation-free in steady state.
                let outcome = graph.run_once_borrowed(self.pool);
                if let Some(source) = outcome.error {
                    #[allow(clippy::unwrap_used)]
                    // fail-fast: poison unreachable — the lock is held only over an infallible Option insert/take, and any holder panic aborts the process before another thread observes it (ADR_0065)
                    let mut g = self.iter_err.lock().unwrap();
                    if g.is_none() {
                        *g = Some(ExecutorError::Item {
                            task_id: task.id.clone(),
                            source,
                        });
                    }
                }
                let _ = outcome.stopped_chain; // chain-abort semantics: no extra bookkeeping at task level
            }
        }
    }
}

/// Wraps a `*mut dyn ExecutableItem` so it can cross thread boundaries inside
/// `Pool::submit`. The send is safe because:
///   1. The executor guarantees at most one invocation of a given item at a
///      time (via `pool.barrier()` before the pointer is reused).
///   2. `ExecutableItem: Send`, so moving the pointee across threads is sound
///      when no aliasing exists.
#[allow(unsafe_code)]
struct SendItemPtr {
    ptr: *mut dyn ExecutableItem,
}

impl SendItemPtr {
    fn new(ptr: *mut dyn ExecutableItem) -> Self {
        Self { ptr }
    }

    /// Returns the raw pointer. Takes `&self` so the wrapper can be invoked
    /// repeatedly from an `FnMut` dispatch closure (`REQ_0060` requires the
    /// dispatch closure to be reusable across iterations without allocation).
    fn get(&self) -> *mut dyn ExecutableItem {
        self.ptr
    }
}

// SAFETY: see doc comment above. `Sync` is required so the FnMut dispatch
// closure can borrow `&SendItemPtr` per invocation without making the
// closure itself `!Send`.
#[allow(unsafe_code)]
unsafe impl Send for SendItemPtr {}
#[allow(unsafe_code)]
unsafe impl Sync for SendItemPtr {}

/// Wraps a `*mut Vec<Box<dyn ExecutableItem>>` so a chain dispatch
/// closure can iterate the chain's items in place without first
/// collecting them into a freshly-allocated `Vec`. The send is safe
/// for the same reason as [`SendItemPtr`] (see above): the executor
/// holds `&mut self` for the duration of `dispatch_loop`, and the
/// `pool.barrier()` at the end of each callback ensures the closure
/// has finished using this pointer before the Vec could be touched
/// from the `WaitSet` thread again. The Vec is never resized after
/// dispatch begins. Required for `REQ_0060` — chain dispatch must not
/// allocate per iteration.
#[allow(unsafe_code)]
struct SendChainPtr {
    ptr: *mut Vec<Box<dyn ExecutableItem>>,
}

impl SendChainPtr {
    fn new(ptr: *mut Vec<Box<dyn ExecutableItem>>) -> Self {
        Self { ptr }
    }

    fn get(&self) -> *mut Vec<Box<dyn ExecutableItem>> {
        self.ptr
    }
}

// SAFETY: see doc comment above. `Sync` lets the FnMut dispatch closure
// borrow `&SendChainPtr` per invocation while staying `Send`.
#[allow(unsafe_code)]
unsafe impl Send for SendChainPtr {}
#[allow(unsafe_code)]
unsafe impl Sync for SendChainPtr {}

/// Captured state needed by a dispatch closure to perform post-execute
/// fault detection. All fields are `Arc`-shared with the owning
/// `Executor` and `TaskEntry` so the closure can read/write them
/// wait-free from any pool worker thread. `REQ_0070`, `REQ_0071`,
/// `REQ_0102`.
struct FaultDispatchCtx {
    /// Per-task budget. `None` for chain / graph tasks (no per-task
    /// check) — the executor-wide iteration budget still applies.
    task_budget: Option<Duration>,
    /// Per-task fault state (shared with `TaskEntry::fault`).
    task_fault: Arc<FaultAtomic>,
    /// Per-task monotonic overrun counter (shared with
    /// `TaskEntry::overrun_count`). Increments on EVERY budget breach.
    overrun_count: Arc<AtomicU64>,
    /// Executor-wide iteration budget. `None` means no executor-wide
    /// check.
    iteration_budget: Option<Duration>,
    /// Executor-wide fault state (shared with `Executor::exec_fault`).
    exec_fault: Arc<ExecutorFaultAtomic>,
    /// Executor-wide offending-task index storage (shared with
    /// `Executor::exec_fault_task_idx`).
    exec_fault_task_idx: Arc<AtomicU32>,
    /// Executor-wide breached-budget storage (shared with
    /// `Executor::exec_fault_budget_ms`).
    exec_fault_budget_ms: Arc<AtomicU32>,
    /// Index of this task in the executor's task table.
    task_idx_u32: u32,
    /// Executor start time (shared with `Executor::start_time`).
    exec_start: Arc<OnceLock<Instant>>,
    /// Observer for `on_task_fault` / `on_executor_fault` notifications.
    observer: Arc<dyn Observer>,
}

/// Validate a task's collected trigger declarations before it joins the task
/// table (`REQ_0268`). Applied at every add path — single, chain head, and
/// fault-handler main — at the point the `TriggerDecl`s are first available,
/// regardless of [`DispatchMode`] (the rejected shapes are ill-defined in any
/// mode; Legacy is temporary).
///
/// Rejects two shapes:
///
/// 1. **Cyclic AND event-driven** — a task carrying both an `Interval` decl and
///    any listener-backed decl (`Subscriber` / `Deadline` / `RawListener`). Per
///    `REQ_0106` a task is cyclic XOR event-driven: cyclic tasks have a
///    period/lateness, event-driven tasks do not. Allowing both would dispatch
///    and record the task twice in one wake (phase-a event + phase-b grid),
///    desyncing the `FEAT_0038` `cycle_index` join key (`REQ_0107`).
/// 2. **Zero-period interval** — an `Interval(Duration::ZERO)` busy-spins the
///    grid (`GridTimer::next_timeout` returns `0` every wake and `take_due`
///    re-fires without advancing). A zero scan period is nonsensical.
fn validate_decls(id: &TaskId, decls: &[crate::trigger::TriggerDecl]) -> Result<(), ExecutorError> {
    use crate::trigger::TriggerDecl;

    let has_interval = decls.iter().any(|d| matches!(d, TriggerDecl::Interval(_)));
    let has_listener = decls.iter().any(|d| {
        matches!(
            d,
            TriggerDecl::Subscriber { .. }
                | TriggerDecl::Deadline { .. }
                | TriggerDecl::RawListener(_)
        )
    });

    if has_interval && has_listener {
        return Err(ExecutorError::DeclareTriggers(format!(
            "task `{id}` declares both an interval (cyclic) and a listener \
             (event-driven) trigger; a task may be cyclic (interval) or \
             event-driven (listener) but not both — split it into two tasks"
        )));
    }

    // Exactly one scan period per cyclic task (REQ_0002, #93). Two interval
    // decls share the grid epoch in Grid mode (REQ_0268), come due in the same
    // pass, and would submit one borrowed `*mut dyn FnMut` to two pool workers
    // before the single barrier — a data race on the Linux-default path. Reject
    // at the validate_decls chokepoint, which covers both `add` and
    // `add_chain_with_id_boxed`. Multi-listener stays legal.
    let interval_count = decls
        .iter()
        .filter(|d| matches!(d, TriggerDecl::Interval(_)))
        .count();
    if interval_count > 1 {
        return Err(ExecutorError::DeclareTriggers(format!(
            "task `{id}` declares {interval_count} interval triggers; a cyclic \
             task must declare exactly one scan period — split it into separate \
             tasks"
        )));
    }

    if decls
        .iter()
        .any(|d| matches!(d, TriggerDecl::Interval(dur) if dur.is_zero()))
    {
        return Err(ExecutorError::DeclareTriggers(format!(
            "task `{id}` declares a zero-duration interval; a cyclic scan \
             period must be strictly positive"
        )));
    }

    Ok(())
}

/// Extract the declared scan period (first `Interval` trigger) from a task's
/// trigger declarations, or `None` for event-driven tasks.
fn scan_period_from_decls(decls: &[crate::trigger::TriggerDecl]) -> Option<Duration> {
    decls.iter().find_map(|d| match d {
        crate::trigger::TriggerDecl::Interval(dur) => Some(*dur),
        _ => None,
    })
}

/// Build the per-iteration dispatch closure for a `TaskKind::Single`.
///
/// The returned closure is stored on `TaskEntry::job` and invoked once
/// per dispatch via `Pool::submit_borrowed`, which (unlike `submit`)
/// performs no allocation. The closure captures Arc clones of the
/// executor's shared state — those clones are refcount-only at build
/// time and are reused on every dispatch. Required for `REQ_0060`.
#[allow(clippy::too_many_arguments)]
fn build_single_job(
    id: TaskId,
    stop: Stoppable,
    obs: Arc<dyn Observer>,
    mon: Arc<dyn ExecutionMonitor>,
    err_slot: Arc<std::sync::Mutex<Option<ExecutorError>>>,
    app_id: Option<u32>,
    app_inst: Option<u32>,
    item_ptr: SendItemPtr,
    fault_ctx: FaultDispatchCtx,
    last_took_ns: Arc<AtomicU64>,
    clock: Arc<dyn MonotonicClock>,
) -> Box<dyn FnMut() + Send + 'static> {
    Box::new(move || {
        let mut ctx = crate::context::Context::new(&id, &stop, obs.as_ref());
        if let Some(aid) = app_id {
            obs.on_app_start(id.clone(), aid, app_inst);
        }
        let raw = item_ptr.get();
        let started = std::time::Instant::now();
        // Telemetry `took` is measured on the injected clock (REQ_0105) so a
        // MockClock can make it exact; the real `started`/`took` below stay on
        // the system clock for the monitor and fault-budget paths.
        let tele_t0 = clock.now_nanos();
        mon.pre_execute(id.clone(), started);
        // SAFETY: barrier() pairs with this invocation; the WaitSet
        // thread does not touch the item between `submit_borrowed` and
        // the matching `barrier()`. See SendItemPtr safety doc.
        #[allow(unsafe_code)]
        let res = run_item_catch_unwind(unsafe { &mut *raw }, &mut ctx);
        let took = started.elapsed();
        // Release pairs with the WaitSet-thread Acquire (swap) in
        // `record_cycle_for` (M2). `pool.barrier()` also fences, but the
        // explicit pairing documents intent and is robust on weak-memory archs.
        last_took_ns.store(clock.now_nanos().saturating_sub(tele_t0), Ordering::Release);
        mon.post_execute(id.clone(), started, took, res.is_ok());
        if let Err(ref e) = res {
            obs.on_app_error(id.clone(), e.as_ref());
        }
        if app_id.is_some() {
            obs.on_app_stop(id.clone());
        }
        post_execute_detect_fault(&id, started, took, &fault_ctx);
        record_first_err(&err_slot, &id, res);
    })
}

/// Build the per-iteration dispatch closure for a fault-handler item.
///
/// Mirrors [`build_single_job`] in every detail (same monitor /
/// observer / first-error capture wiring) but owns the
/// `Box<dyn ExecutableItem>` directly inside the closure instead of
/// dereferencing a raw [`SendItemPtr`]. The handler has no parallel
/// owner inside [`TaskEntry`] — the handler closure stored in
/// `handler_job` is the sole owner — so the simpler owning form is
/// both sound and avoids the aliasing dance the main item needs.
/// (Unlike [`build_single_job`], this closure does NOT update
/// `last_took_ns` — the handler runs in place of the main item, so the
/// main item's `last_took_ns` keeps its sentinel `u64::MAX` = "no
/// sample this cycle".)
/// `REQ_0072`.
#[allow(clippy::too_many_arguments)]
fn build_handler_job(
    id: TaskId,
    stop: Stoppable,
    obs: Arc<dyn Observer>,
    mon: Arc<dyn ExecutionMonitor>,
    err_slot: Arc<std::sync::Mutex<Option<ExecutorError>>>,
    app_id: Option<u32>,
    app_inst: Option<u32>,
    mut handler: Box<dyn ExecutableItem>,
    fault_ctx: FaultDispatchCtx,
) -> Box<dyn FnMut() + Send + 'static> {
    Box::new(move || {
        let mut ctx = crate::context::Context::new(&id, &stop, obs.as_ref());
        if let Some(aid) = app_id {
            obs.on_app_start(id.clone(), aid, app_inst);
        }
        let started = std::time::Instant::now();
        mon.pre_execute(id.clone(), started);
        let res = run_item_catch_unwind(handler.as_mut(), &mut ctx);
        let took = started.elapsed();
        mon.post_execute(id.clone(), started, took, res.is_ok());
        if let Err(ref e) = res {
            obs.on_app_error(id.clone(), e.as_ref());
        }
        if app_id.is_some() {
            obs.on_app_stop(id.clone());
        }
        // Per §4.6 invariant 5 of FEAT_0018: a handler that ALSO breaches
        // budget keeps the task in `Faulted` (state already `Faulted`),
        // `overrun_count` increments, NO new `on_task_fault` fires —
        // the `matches!(prev, FaultState::Running)` gate inside
        // `post_execute_detect_fault` enforces that.
        post_execute_detect_fault(&id, started, took, &fault_ctx);
        record_first_err(&err_slot, &id, res);
    })
}

/// Build the per-iteration dispatch closure for a `TaskKind::Chain`.
#[allow(clippy::too_many_arguments)]
fn build_chain_job(
    id: TaskId,
    stop: Stoppable,
    obs: Arc<dyn Observer>,
    mon: Arc<dyn ExecutionMonitor>,
    err_slot: Arc<std::sync::Mutex<Option<ExecutorError>>>,
    chain_ptr: SendChainPtr,
    fault_ctx: FaultDispatchCtx,
    last_took_ns: Arc<AtomicU64>,
    clock: Arc<dyn MonotonicClock>,
) -> Box<dyn FnMut() + Send + 'static> {
    Box::new(move || {
        let mut ctx = crate::context::Context::new(&id, &stop, obs.as_ref());
        // Overall chain scan timer — the chain's `took` is the elapsed
        // telemetry-clock time from the first item's pre-execute to the last
        // item's completion (or early break), mirroring the single-item `took`
        // notion (REQ_0105). Per-item monitor timing uses each item's own
        // real-clock `started` below.
        let chain_tele_t0 = clock.now_nanos();
        // SAFETY: barrier() pairs with this invocation; the chain Vec
        // and the items it owns are not touched by the WaitSet thread
        // until barrier() returns. See SendChainPtr safety doc.
        #[allow(unsafe_code)]
        let chain_items = unsafe { &mut *chain_ptr.get() };
        for item_box in chain_items.iter_mut() {
            let app_id = item_box.app_id();
            let app_inst = item_box.app_instance_id();
            if let Some(aid) = app_id {
                obs.on_app_start(id.clone(), aid, app_inst);
            }
            let raw = std::ptr::from_mut::<dyn ExecutableItem>(item_box.as_mut());
            let started = std::time::Instant::now();
            mon.pre_execute(id.clone(), started);
            #[allow(unsafe_code)]
            let res = run_item_catch_unwind(unsafe { &mut *raw }, &mut ctx);
            let took = started.elapsed();
            mon.post_execute(id.clone(), started, took, res.is_ok());
            if let Err(ref e) = res {
                obs.on_app_error(id.clone(), e.as_ref());
            }
            if app_id.is_some() {
                obs.on_app_stop(id.clone());
            }
            // Per-item post-execute fault detection. `task_budget` is
            // `None` for chains (see `add_chain_with_id_boxed`), so the
            // per-task check no-ops; the executor-wide iteration-budget
            // check still fires per item. `REQ_0071`.
            post_execute_detect_fault(&id, started, took, &fault_ctx);
            match res {
                Ok(crate::ControlFlow::Continue) => {}
                Ok(crate::ControlFlow::StopChain) => break,
                Err(_) => {
                    record_first_err(&err_slot, &id, res);
                    break;
                }
            }
        }
        // Release pairs with the WaitSet-thread Acquire (swap) in
        // `record_cycle_for` (M2). See the Single-job store for the rationale.
        last_took_ns.store(
            clock.now_nanos().saturating_sub(chain_tele_t0),
            Ordering::Release,
        );
    })
}

#[derive(Debug)]
struct PanickedTask(String);

impl core::fmt::Display for PanickedTask {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "task panicked: {}", self.0)
    }
}

impl std::error::Error for PanickedTask {}

/// Execute `item` inside `catch_unwind`, converting any panic into an `Err`.
fn run_item_catch_unwind(
    item: &mut dyn ExecutableItem,
    ctx: &mut crate::context::Context<'_>,
) -> crate::ExecuteResult {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| item.execute(ctx))).unwrap_or_else(
        |payload| {
            let msg =
                panic_payload_message(&*payload).unwrap_or_else(|| "panicked task".to_string());
            Err::<crate::ControlFlow, crate::ItemError>(Box::new(PanickedTask(msg)))
        },
    )
}

/// Public-within-crate wrapper so `graph.rs` can call `run_item_catch_unwind`
/// without depending on its private name.
pub(crate) fn run_item_catch_unwind_external(
    item: &mut dyn ExecutableItem,
    ctx: &mut crate::context::Context<'_>,
) -> crate::ExecuteResult {
    run_item_catch_unwind(item, ctx)
}

/// Record the first error into `slot`. Subsequent errors are silently dropped.
fn record_first_err(
    slot: &Arc<std::sync::Mutex<Option<ExecutorError>>>,
    id: &TaskId,
    res: crate::ExecuteResult,
) {
    if let Err(source) = res {
        let mut g = slot.lock().unwrap();
        if g.is_none() {
            *g = Some(ExecutorError::Item {
                task_id: id.clone(),
                source,
            });
        }
    }
}

/// Post-execute fault detection — runs on a pool worker AFTER
/// `mon.post_execute` so the full `took` is available. Implements:
///
///   * `REQ_0070` / `REQ_0102` — per-task budget overrun: increments
///     `overrun_count` on every breach, transitions
///     `Running -> Faulted{BudgetExceeded}` exactly once (subsequent
///     breaches keep the state `Faulted` and do NOT re-fire the
///     observer).
///   * `REQ_0071` — executor-wide iteration overrun: transitions
///     `Running -> Faulted{IterationBudgetExceeded}` exactly once;
///     cascade to per-task state is LAZY (see the pre-dispatch block
///     in `dispatch_loop`), so the per-task `on_task_fault` does NOT
///     fire during cascade — only `on_executor_fault` does.
fn post_execute_detect_fault(
    id: &TaskId,
    started: Instant,
    took: Duration,
    fault_ctx: &FaultDispatchCtx,
) {
    // REQ_0070 / REQ_0102 — per-task budget overrun.
    if let Some(budget) = fault_ctx.task_budget {
        if took > budget {
            fault_ctx.overrun_count.fetch_add(1, Ordering::Relaxed);
            let took_ms = duration_to_ms_sat(took);
            let budget_ms = duration_to_ms_sat(budget);
            let exec_start = *fault_ctx.exec_start.get_or_init(|| started);
            let since_ms = instant_to_since_ms(started, exec_start);
            let new_state = FaultState::Faulted {
                reason: FaultReason::BudgetExceeded { took_ms, budget_ms },
                since_ms,
            };
            let prev = fault_ctx.task_fault.swap(new_state, budget_ms);
            if matches!(prev, FaultState::Running) {
                fault_ctx.observer.on_task_fault(
                    id.clone(),
                    FaultReason::BudgetExceeded { took_ms, budget_ms },
                );
            }
        }
    }

    // REQ_0071 — executor-wide iteration overrun.
    if let Some(iter_budget) = fault_ctx.iteration_budget {
        if took > iter_budget {
            let took_ms = duration_to_ms_sat(took);
            let budget_ms = duration_to_ms_sat(iter_budget);
            let exec_start = *fault_ctx.exec_start.get_or_init(|| started);
            let since_ms = instant_to_since_ms(started, exec_start);
            fault_ctx
                .exec_fault_task_idx
                .store(fault_ctx.task_idx_u32, Ordering::Release);
            fault_ctx
                .exec_fault_budget_ms
                .store(budget_ms, Ordering::Release);
            let new_state = ExecutorFaultState::Faulted {
                reason: ExecutorFaultReason::IterationBudgetExceeded {
                    task_idx: fault_ctx.task_idx_u32,
                    took_ms,
                    budget_ms,
                },
                since_ms,
            };
            let prev = fault_ctx
                .exec_fault
                .swap(new_state, fault_ctx.task_idx_u32, budget_ms);
            if matches!(prev, ExecutorFaultState::Running) {
                fault_ctx.observer.on_executor_fault(
                    ExecutorFaultReason::IterationBudgetExceeded {
                        task_idx: fault_ctx.task_idx_u32,
                        took_ms,
                        budget_ms,
                    },
                );
                // NO eager cascade here. Cascade is lazy: the
                // pre-dispatch block in `dispatch_loop` transitions
                // each `Running` task to `Faulted{ExecutorFaulted}` on
                // the next wakeup — silently, so per-task observers
                // do not fire (see §4.6 invariant on cascade-noise).
            }
        }
    }
}

// ── ExecutorGraphBuilder ──────────────────────────────────────────────────────

/// Borrowed wrapper that finalises a [`GraphBuilder`](crate::graph::GraphBuilder)
/// into a registered task.
pub struct ExecutorGraphBuilder<'e> {
    executor: &'e mut Executor,
    builder: crate::graph::GraphBuilder,
    custom_id: Option<TaskId>,
}

impl ExecutorGraphBuilder<'_> {
    /// Add a vertex to the graph; returns its handle.
    pub fn vertex<I: ExecutableItem>(&mut self, item: I) -> crate::graph::Vertex {
        self.builder.vertex(item)
    }

    /// Add a directed edge from one vertex to another.
    pub fn edge(&mut self, from: crate::graph::Vertex, to: crate::graph::Vertex) -> &mut Self {
        self.builder.edge(from, to);
        self
    }

    /// Designate the root vertex (its triggers gate the graph).
    pub const fn root(&mut self, v: crate::graph::Vertex) -> &mut Self {
        self.builder.root(v);
        self
    }

    /// Override the auto-generated id with a custom one.
    pub fn id(&mut self, id: impl Into<TaskId>) -> &mut Self {
        self.custom_id = Some(id.into());
        self
    }

    /// Validate and register the graph. Returns the task id.
    ///
    /// The root vertex's [`ExecutableItem::task_id`] override takes precedence
    /// over any id set via [`ExecutorGraphBuilder::id`], which itself takes
    /// precedence over the auto-generated id.
    pub fn build(self) -> Result<TaskId, ExecutorError> {
        let g = self.builder.finish()?;
        // Root vertex's task_id() override wins over the custom id, which wins
        // over the auto-generated fallback.
        let auto_id = || {
            TaskId::new(format!(
                "graph-{}",
                self.executor.next_id.fetch_add(1, Ordering::SeqCst)
            ))
        };
        let id = g
            .root_task_id()
            .map(TaskId::new)
            .or(self.custom_id)
            .unwrap_or_else(auto_id);
        let decls = g.decls.clone();
        // The graph root's decls become a grid-registered TaskEntry, so the same
        // cyclic-XOR-event-driven / non-zero-period validation that guards the
        // single-item, fault-handler, and chain add paths must guard this one too
        // (REQ_0268). Non-root vertex triggers never reach a TaskEntry — they are
        // discarded in `GraphBuilder::collect_root_decls` — so validating the root
        // decls is sufficient.
        validate_decls(&id, &decls)?;
        let scan_period = scan_period_from_decls(&decls);

        // Box the graph for address stability — per-vertex dispatch
        // closures capture `*const Graph` and must not see it move.
        let mut graph_box: Box<crate::graph::Graph> = Box::new(g);
        // Pre-build the per-vertex closures now that we know the
        // task_id and have access to the executor's shared state.
        graph_box.prepare_dispatch(
            id.clone(),
            self.executor.stoppable.clone(),
            Arc::clone(&self.executor.observer),
            Arc::clone(&self.executor.monitor),
            Arc::clone(&self.executor.iter_err),
        );

        self.executor.tasks.push(TaskEntry {
            id: id.clone(),
            kind: TaskKind::Graph(graph_box),
            decls,
            // Graph tasks dispatch their vertices via `vertex_jobs`
            // stored inside the `Graph`; the per-task `job` slot
            // is unused for graphs.
            job: None,
            // TODO(post-Task-10): graph budgets carried separately; for now None.
            budget: None,
            fault: Arc::new(FaultAtomic::new()),
            overrun_count: Arc::new(AtomicU64::new(0)),
            handler_job: None,
            scan_period,
            // Graphs dispatch vertices via their own path and do not ferry a
            // per-task `took`; sentinel = "no sample". Wired for struct
            // completeness; nothing reads it yet (Task 6).
            last_took_ns: Arc::new(AtomicU64::new(u64::MAX)),
            last_dispatch: None,
            grid_slot: 0,
            grid_epoch: None,
            pending_skipped: 0,
            pending_late: None,
            pending_cycle: None,
        });
        self.executor
            .cycle_stats
            .push(TaskCycleStats::new(self.executor.stats_window));
        Ok(id)
    }
}

// ── Test seam ─────────────────────────────────────────────────────────────────

#[cfg(test)]
impl Executor {
    /// White-box seam (`#93`, `REQ_0854`): reproduce the batched-barrier / grid
    /// wake — two dispatches of one task with a SINGLE barrier between them —
    /// over a guard-less [`DispatchPass`] built from our own fields.
    /// `dispatch_task` / `barrier_and_record` never read `guards` /
    /// `attachment_to_task`, so empty slices suffice; `stop_listener_ptr` points
    /// at the real Arc. Pins the per-phase dedup contract: the second dispatch
    /// must be skipped, so the borrowed job (main item or fault handler) is
    /// submitted at most once — never aliased across two pool workers.
    #[allow(unsafe_code, clippy::ref_as_ptr, clippy::borrow_as_ptr)]
    fn dispatch_twice_one_barrier(&mut self, task_idx: usize) {
        // Raw pointers taken first so the &mut borrows are released before the
        // shared borrows below — same discipline as `dispatch_loop`.
        let tasks_ptr = &mut self.tasks as *mut Vec<TaskEntry>;
        let cycle_stats_ptr = &mut self.cycle_stats as *mut Vec<TaskCycleStats>;
        let exec_fault_ptr = &*self.exec_fault as *const ExecutorFaultAtomic;
        let exec_start_ptr = &*self.start_time as *const OnceLock<Instant>;
        let stop_listener_ptr = self.stop_listener.as_ref() as *const IxListener<ipc::Service>;
        let observer = &self.observer;
        let pool = &self.pool;
        let clock = &self.clock;
        let iter_err = Arc::clone(&self.iter_err);
        let mut pass = DispatchPass {
            guards: &[],
            attachment_to_task: &[],
            tasks_ptr,
            cycle_stats_ptr,
            observer,
            exec_fault_ptr,
            exec_start_ptr,
            clock,
            stop_listener_ptr,
            pool,
            iter_err: &iter_err,
        };
        pass.dispatch_task(task_idx);
        pass.dispatch_task(task_idx);
        pass.barrier_and_record();
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ControlFlow, item};
    use iceoryx2::prelude::ZeroCopySend;

    /// Minimal zero-copy payload for tests that need a real subscriber to
    /// produce a listener-backed trigger decl.
    #[derive(Debug, Default, Clone, Copy, ZeroCopySend)]
    #[repr(C)]
    struct Msg(u32);

    #[test]
    fn add_returns_unique_ids() {
        let mut exec = Executor::builder().worker_threads(0).build().unwrap();
        let a = exec.add(item(|_| Ok(ControlFlow::Continue))).unwrap();
        let b = exec.add(item(|_| Ok(ControlFlow::Continue))).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn dispatch_guard_runs_event_task_once_per_phase() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU64, Ordering};
        // REQ_0854 / #93: two dispatches of one task with a single barrier
        // between them (the future batched-barrier / grid pattern) must submit
        // the borrowed main job at most ONCE — re-submitting the same
        // `*mut dyn FnMut` would alias it across two pool workers. An event task
        // (no scan period) exercises the normal `submit_task_job` path.
        let runs = Arc::new(AtomicU64::new(0));
        let r = Arc::clone(&runs);
        let mut exec = Executor::builder().worker_threads(0).build().unwrap();
        exec.add(item(move |_| {
            r.fetch_add(1, Ordering::Relaxed);
            Ok(ControlFlow::Continue)
        }))
        .expect("add");
        exec.dispatch_twice_one_barrier(0);
        assert_eq!(
            runs.load(Ordering::Relaxed),
            1,
            "borrowed main job must be submitted exactly once per barrier phase"
        );
    }

    #[test]
    fn dispatch_guard_runs_fault_handler_once_per_phase() {
        use crate::fault::{FaultReason, FaultState};
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU64, Ordering};
        // REQ_0854 / #93: the dedup token must cover the BORROWED FAULT HANDLER
        // submit, not just the main job. An event task (no scan period) routed
        // to its handler twice in one barrier phase must run the handler once.
        // Before the uniform-token fix, the fault branch only set `pending_cycle`
        // for cyclic tasks, so an event task's handler aliased across workers.
        let runs = Arc::new(AtomicU64::new(0));
        let r = Arc::clone(&runs);
        let mut exec = Executor::builder().worker_threads(0).build().unwrap();
        exec.add_with_fault_handler(
            item(|_| Ok(ControlFlow::Continue)),
            item(move |_| {
                r.fetch_add(1, Ordering::Relaxed);
                Ok(ControlFlow::Continue)
            }),
        )
        .expect("add_with_fault_handler");
        // Drive the task to Faulted so dispatch routes to the handler.
        exec.tasks[0].fault.swap(
            FaultState::Faulted {
                reason: FaultReason::BudgetExceeded {
                    took_ms: 6,
                    budget_ms: 2,
                },
                since_ms: 0,
            },
            0,
        );
        exec.dispatch_twice_one_barrier(0);
        assert_eq!(
            runs.load(Ordering::Relaxed),
            1,
            "borrowed fault handler must be submitted exactly once per barrier phase"
        );
    }

    #[test]
    fn grid_mode_dispatches_cyclic_task_each_cycle() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU64, Ordering};
        let hits = Arc::new(AtomicU64::new(0));
        let h = Arc::clone(&hits);
        let mut exec = Executor::builder()
            .worker_threads(0)
            .dispatch_mode(crate::DispatchMode::Grid)
            .build()
            .expect("build");
        exec.add(crate::item::item_with_triggers(
            move |d| {
                d.interval(std::time::Duration::from_millis(1));
                Ok(())
            },
            move |_ctx| {
                h.fetch_add(1, Ordering::Relaxed);
                Ok(ControlFlow::Continue)
            },
        ))
        .expect("add");
        exec.run_n(10).expect("run");
        assert!(
            hits.load(Ordering::Relaxed) >= 8,
            "grid mode under-dispatched: {}",
            hits.load(Ordering::Relaxed)
        );
    }

    #[test]
    fn legacy_mode_dispatches_cyclic_task_each_cycle() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU64, Ordering};
        let hits = Arc::new(AtomicU64::new(0));
        let h = Arc::clone(&hits);
        let mut exec = Executor::builder()
            .worker_threads(0)
            .dispatch_mode(crate::DispatchMode::Legacy)
            .build()
            .expect("build");
        exec.add(crate::item::item_with_triggers(
            move |d| {
                d.interval(std::time::Duration::from_millis(1));
                Ok(())
            },
            move |_ctx| {
                h.fetch_add(1, Ordering::Relaxed);
                Ok(ControlFlow::Continue)
            },
        ))
        .expect("add");
        exec.run_n(10).expect("run");
        assert!(
            hits.load(Ordering::Relaxed) >= 8,
            "legacy mode under-dispatched: {}",
            hits.load(Ordering::Relaxed)
        );
    }

    #[test]
    fn stranded_pending_cycle_token_does_not_swallow_first_dispatch() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU32, Ordering};
        // Regression for #102: the `dispatch_loop` entry sweep clears the three
        // per-wake transients (REQ_0854, #93) so a prior `run_*` that bailed
        // mid-wake cannot strand them. This test presets ALL THREE on the task
        // and asserts the two that are first-cycle observable under `run_n(1)`:
        //
        //   * `pending_cycle = Some(..)` — doubles as the per-phase dispatch
        //     dedup token. `dispatch_task`'s `pending_cycle.is_some()` guard
        //     would silently swallow the task's first dispatch if the sweep
        //     didn't clear it (the dispatch-count assertion guards this clear).
        //   * `pending_late = Some(L)` — `record_cycle_for` back-dates the
        //     first-cycle `grid_epoch` via `pre_ns.saturating_sub(L)` (the
        //     `get_or_insert_with` anchor). `pre_ns` is ns since the clock's
        //     build epoch, so this early in a run it's small (~1 ms); with
        //     `L = 5_000_000 > pre_ns` the subtraction saturates to 0, so the
        //     first recorded cycle's `lateness_ns` reads a nonzero value
        //     (`≈ pre_ns`, the dispatch instant, in this saturating regime; it
        //     would equal `L` only once `pre_ns >= L`) instead of the clean
        //     `0` — a silently corrupted lateness baseline for the whole run
        //     (the lateness assertion guards this clear).
        //   * `pending_skipped = <nonzero>` — `mem::take`n by the same record
        //     pass, but the first record has `first == true`, so it does NOT
        //     advance `grid_slot` and has no first-cycle observable effect.
        //     Its clear is defensive (it would only bite a later cycle), so
        //     there is intentionally no assertion on it here; a `run_n(2)`
        //     test for it would be flaky on real timers. Presetting it is for
        //     completeness, to prove the sweep clears all three together.
        //
        // The sweep makes the token lifetime one `dispatch_loop` invocation by
        // construction even though `Executor` is `&mut self` and `self.tasks`
        // persists across `run_*` calls.
        struct LatenessRecorder {
            lateness: std::sync::Mutex<Vec<Option<i64>>>,
        }
        impl Observer for LatenessRecorder {
            fn on_cycle_stats(&self, obs: &CycleObservation) {
                self.lateness.lock().expect("lock").push(obs.lateness_ns);
            }
        }

        let hits = Arc::new(AtomicU32::new(0));
        let h = Arc::clone(&hits);
        let recorder = Arc::new(LatenessRecorder {
            lateness: std::sync::Mutex::new(Vec::new()),
        });
        // Legacy mode is mandatory for scripted/deterministic cyclic tests: the
        // platform default races on macOS and skips the first Grid cycle on
        // ubuntu.
        let mut exec = Executor::builder()
            .worker_threads(0)
            .dispatch_mode(crate::DispatchMode::Legacy)
            .observer(Arc::clone(&recorder) as Arc<dyn Observer>)
            .build()
            .expect("build");
        exec.add(crate::item::item_with_triggers(
            move |d| {
                d.interval(std::time::Duration::from_millis(1));
                Ok(())
            },
            move |_ctx| {
                h.fetch_add(1, Ordering::Relaxed);
                Ok(ControlFlow::Continue)
            },
        ))
        .expect("add");

        // Simulate the stranded-token precondition: a prior `dispatch_loop`
        // bailed mid-wake after marking the task but before folding it, leaving
        // all three per-wake transients `Some`/nonzero.
        exec.tasks[0].pending_cycle = Some(CyclePending {
            pre: 0,
            faulted: false,
        });
        exec.tasks[0].pending_skipped = 7;
        exec.tasks[0].pending_late = Some(5_000_000);

        exec.run_n(1).expect("run");

        // Guards the `pending_cycle = None` clear: the handler fires exactly
        // once. Without the clear, the dedup guard swallows the first dispatch.
        assert_eq!(
            hits.load(Ordering::Relaxed),
            1,
            "stale pending_cycle token swallowed the first dispatch (#102)"
        );

        // Guards the `pending_late = None` clear. First cycle ⇒ `grid_slot = 0`,
        // `expected = 0`, `elapsed = pre - grid_epoch`. With the clear,
        // `late_by = None` ⇒ `grid_epoch = pre` ⇒ lateness `Some(0)`. Without it,
        // `late_by = Some(5_000_000)` ⇒ `grid_epoch = pre.saturating_sub(L)`,
        // which saturates to 0 here (`L > pre`, `pre` being ns since the clock
        // epoch), so lateness reads a nonzero `Some(≈ pre)` — the dispatch
        // instant, not `L` (it would read `Some(L)` only once `pre >= L`). The
        // assertion turns only on clean-being-exactly-`Some(0)` vs corrupted-
        // being-nonzero, not on the corrupted magnitude.
        let first_lateness = *recorder
            .lateness
            .lock()
            .expect("lock")
            .first()
            .expect("at least one cycle observation must have been recorded");
        assert_eq!(
            first_lateness,
            Some(0),
            "stale pending_late token corrupted the first-cycle lateness baseline (#102)"
        );
    }

    // --- REQ_0268 trigger-combination validation (Fix 1 / Fix 3) ---

    #[test]
    fn add_rejects_cyclic_plus_subscriber_combination() {
        use core::time::Duration;
        // A task declaring BOTH an Interval and a listener-backed trigger is
        // ill-defined (cyclic XOR event-driven, REQ_0106) and must be rejected
        // at add time. We use a real subscriber so the listener decl is genuine.
        let mut exec = Executor::builder().worker_threads(0).build().unwrap();
        let ch = exec.channel::<Msg>("taktora.test.req0268.combo").unwrap();
        let sub = ch.subscriber().unwrap();
        let err = exec
            .add(crate::item::item_with_triggers(
                move |d| {
                    d.interval(Duration::from_millis(1));
                    d.subscriber(&sub);
                    Ok(())
                },
                |_| Ok(crate::ControlFlow::Continue),
            ))
            .expect_err("interval + subscriber must be rejected");
        match err {
            ExecutorError::DeclareTriggers(msg) => {
                assert!(
                    msg.contains("cyclic") && msg.contains("event-driven"),
                    "message must explain cyclic vs event-driven: {msg}"
                );
                assert!(
                    msg.contains("split"),
                    "message must suggest splitting into two tasks: {msg}"
                );
            }
            other => panic!("expected DeclareTriggers, got {other:?}"),
        }
    }

    #[test]
    fn add_rejects_cyclic_plus_listener_regardless_of_mode() {
        use core::time::Duration;
        // The combination is ill-defined irrespective of DispatchMode (Legacy
        // is temporary), so Legacy must reject it too.
        let mut exec = Executor::builder()
            .worker_threads(0)
            .dispatch_mode(crate::DispatchMode::Legacy)
            .build()
            .unwrap();
        let ch = exec
            .channel::<Msg>("taktora.test.req0268.combo.legacy")
            .unwrap();
        let sub = ch.subscriber().unwrap();
        let err = exec
            .add(crate::item::item_with_triggers(
                move |d| {
                    d.interval(Duration::from_millis(1));
                    d.subscriber(&sub);
                    Ok(())
                },
                |_| Ok(crate::ControlFlow::Continue),
            ))
            .expect_err("interval + subscriber must be rejected in Legacy too");
        assert!(matches!(err, ExecutorError::DeclareTriggers(_)));
    }

    #[test]
    fn add_accepts_single_interval_and_multiple_listeners() {
        use core::time::Duration;
        let mut exec = Executor::builder().worker_threads(0).build().unwrap();
        // Single interval: accepted (the only legal cyclic shape — REQ_0002).
        exec.add(crate::item::item_with_triggers(
            |d| {
                d.interval(Duration::from_millis(1));
                Ok(())
            },
            |_| Ok(crate::ControlFlow::Continue),
        ))
        .expect("single interval accepted");
        // Multiple listeners (no interval): still accepted (multi-listener is
        // legal; only multi-interval is rejected — #93).
        let ch = exec
            .channel::<Msg>("taktora.test.req0268.multi.listener")
            .unwrap();
        let sub_a = ch.subscriber().unwrap();
        let sub_b = ch.subscriber().unwrap();
        exec.add(crate::item::item_with_triggers(
            move |d| {
                d.subscriber(&sub_a);
                d.subscriber(&sub_b);
                Ok(())
            },
            |_| Ok(crate::ControlFlow::Continue),
        ))
        .expect("multiple listeners accepted");
    }

    #[test]
    fn add_rejects_multiple_intervals() {
        use core::time::Duration;
        // Two interval() decls on one task: in Grid mode (Linux default) both
        // share the grid epoch, come due in the same pass, and would submit one
        // borrowed FnMut to two pool workers before the single barrier — a data
        // race. A cyclic task must declare exactly one scan period (REQ_0002),
        // so reject at add() time. (#93)
        let mut exec = Executor::builder().worker_threads(0).build().unwrap();
        let err = exec
            .add(crate::item::item_with_triggers(
                |d| {
                    d.interval(Duration::from_millis(1));
                    d.interval(Duration::from_millis(2));
                    Ok(())
                },
                |_| Ok(crate::ControlFlow::Continue),
            ))
            .expect_err("multiple intervals must be rejected");
        match err {
            ExecutorError::DeclareTriggers(msg) => {
                assert!(
                    msg.contains("interval"),
                    "message must mention the interval triggers: {msg}"
                );
            }
            other => panic!("expected DeclareTriggers, got {other:?}"),
        }
    }

    #[test]
    fn add_rejects_zero_period_interval() {
        use core::time::Duration;
        // A zero-period interval busy-spins the grid (next_timeout == 0 every
        // wake), so it must be rejected at add time.
        let mut exec = Executor::builder().worker_threads(0).build().unwrap();
        let err = exec
            .add(crate::item::item_with_triggers(
                |d| {
                    d.interval(Duration::ZERO);
                    Ok(())
                },
                |_| Ok(crate::ControlFlow::Continue),
            ))
            .expect_err("zero-period interval must be rejected");
        match err {
            ExecutorError::DeclareTriggers(msg) => {
                assert!(
                    msg.contains("zero"),
                    "message must mention the zero period: {msg}"
                );
            }
            other => panic!("expected DeclareTriggers, got {other:?}"),
        }
    }

    #[test]
    fn add_chain_rejects_cyclic_plus_listener() {
        use core::time::Duration;
        // The chain path collects the head item's decls; the same validation
        // must apply there.
        let mut exec = Executor::builder().worker_threads(0).build().unwrap();
        let ch = exec
            .channel::<Msg>("taktora.test.req0268.chain.combo")
            .unwrap();
        let sub = ch.subscriber().unwrap();
        let err = exec
            .add_chain(vec![crate::item::item_with_triggers(
                move |d| {
                    d.interval(Duration::from_millis(1));
                    d.subscriber(&sub);
                    Ok(())
                },
                |_| Ok(crate::ControlFlow::Continue),
            )])
            .expect_err("chain head interval + subscriber must be rejected");
        assert!(matches!(err, ExecutorError::DeclareTriggers(_)));
    }

    #[test]
    fn add_chain_rejects_zero_period_interval() {
        use core::time::Duration;
        let mut exec = Executor::builder().worker_threads(0).build().unwrap();
        let err = exec
            .add_chain(vec![crate::item::item_with_triggers(
                |d| {
                    d.interval(Duration::ZERO);
                    Ok(())
                },
                |_| Ok(crate::ControlFlow::Continue),
            )])
            .expect_err("chain head zero-period interval must be rejected");
        assert!(matches!(err, ExecutorError::DeclareTriggers(_)));
    }

    #[test]
    fn add_chain_rejects_multiple_intervals() {
        use core::time::Duration;
        // The chain path collects the head item's decls through the same
        // validate_decls chokepoint, so the one-scan-period rule (REQ_0002, #93)
        // covers chains for free — pin it.
        let mut exec = Executor::builder().worker_threads(0).build().unwrap();
        let err = exec
            .add_chain(vec![crate::item::item_with_triggers(
                |d| {
                    d.interval(Duration::from_millis(1));
                    d.interval(Duration::from_millis(2));
                    Ok(())
                },
                |_| Ok(crate::ControlFlow::Continue),
            )])
            .expect_err("chain head with two intervals must be rejected");
        match err {
            ExecutorError::DeclareTriggers(msg) => assert!(
                msg.contains("interval"),
                "message must mention the interval triggers: {msg}"
            ),
            other => panic!("expected DeclareTriggers, got {other:?}"),
        }
    }

    #[test]
    fn add_graph_rejects_cyclic_plus_listener() {
        use core::time::Duration;
        // The graph path collects the root vertex's decls into a grid-registered
        // TaskEntry; the same cyclic-XOR-event-driven validation must apply there
        // (REQ_0268). We use a real subscriber so the listener decl is genuine.
        let mut exec = Executor::builder().worker_threads(0).build().unwrap();
        let ch = exec
            .channel::<Msg>("taktora.test.req0268.graph.combo")
            .unwrap();
        let sub = ch.subscriber().unwrap();
        let mut g = exec.add_graph();
        let r = g.vertex(crate::item::item_with_triggers(
            move |d| {
                d.interval(Duration::from_millis(1));
                d.subscriber(&sub);
                Ok(())
            },
            |_| Ok(crate::ControlFlow::Continue),
        ));
        g.root(r);
        let err = g
            .build()
            .expect_err("graph root interval + subscriber must be rejected");
        assert!(matches!(err, ExecutorError::DeclareTriggers(_)));
    }

    #[test]
    fn add_graph_rejects_zero_period_interval() {
        use core::time::Duration;
        // A zero-period interval on the graph root busy-spins the grid, so the
        // graph path must reject it just like the single-item/chain paths.
        let mut exec = Executor::builder().worker_threads(0).build().unwrap();
        let mut g = exec.add_graph();
        let r = g.vertex(crate::item::item_with_triggers(
            |d| {
                d.interval(Duration::ZERO);
                Ok(())
            },
            |_| Ok(crate::ControlFlow::Continue),
        ));
        g.root(r);
        let err = g
            .build()
            .expect_err("graph root zero-period interval must be rejected");
        assert!(matches!(err, ExecutorError::DeclareTriggers(_)));
    }

    #[test]
    fn stopped_iteration_emits_no_cyclic_cycle_observation() {
        use core::time::Duration;
        use std::sync::atomic::AtomicU64;

        // A CyclicClock that starts at 0 (epoch) then jumps far past the first
        // grid target, so the post-wait `take_due` finds the cyclic task due on
        // the very first (stopping) wake. Distinct from the telemetry clock
        // (scheduling role).
        struct JumpClock {
            calls: AtomicU64,
        }
        impl crate::CyclicClock for JumpClock {
            fn now_nanos(&self) -> u64 {
                // First read (grid epoch at loop entry) = 0; every later read
                // is well past the 1ms target.
                if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    0
                } else {
                    1_000_000_000
                }
            }
        }

        // Observer that counts on_cycle_stats calls.
        struct Counter {
            cycles: AtomicU64,
        }
        impl Observer for Counter {
            fn on_cycle_stats(&self, _obs: &CycleObservation) {
                self.cycles.fetch_add(1, Ordering::SeqCst);
            }
        }

        let counter = Arc::new(Counter {
            cycles: AtomicU64::new(0),
        });
        let mut exec = Executor::builder()
            .worker_threads(0)
            .dispatch_mode(crate::DispatchMode::Grid)
            .cyclic_clock(Arc::new(JumpClock {
                calls: AtomicU64::new(0),
            }))
            .observer(Arc::clone(&counter) as Arc<dyn Observer>)
            .build()
            .unwrap();
        exec.add(crate::item::item_with_triggers(
            |d| {
                d.interval(Duration::from_millis(1));
                Ok(())
            },
            |_| Ok(crate::ControlFlow::Continue),
        ))
        .unwrap();

        // Stop BEFORE running: the WaitSet wakes immediately on the stop
        // listener; the grid target is already due (JumpClock). Without the
        // stop guard the post-wait cyclic pass would dispatch + record one
        // spurious cycle on this stopping iteration; with it, zero.
        exec.stoppable().stop();
        exec.run().expect("run returns cleanly after stop");

        assert_eq!(
            counter.cycles.load(Ordering::SeqCst),
            0,
            "no cyclic cycle observation may be emitted on a stop wake"
        );
    }

    #[test]
    fn custom_id_is_preserved() {
        let mut exec = Executor::builder().worker_threads(0).build().unwrap();
        let id = exec
            .add_with_id("my-task", item(|_| Ok(ControlFlow::Continue)))
            .unwrap();
        assert_eq!(id.as_str(), "my-task");
    }

    #[test]
    fn add_persists_declared_budget() {
        use core::time::Duration;
        let mut exec = Executor::builder().worker_threads(0).build().unwrap();
        let task_id = exec
            .add(crate::item::item_with_triggers(
                |d| {
                    d.interval(Duration::from_millis(10));
                    d.budget(Duration::from_millis(5));
                    Ok(())
                },
                |_| Ok(crate::ControlFlow::Continue),
            ))
            .unwrap();
        let entry = exec
            .tasks
            .iter()
            .find(|t| t.id == task_id)
            .expect("task present");
        assert_eq!(entry.budget, Some(Duration::from_millis(5)));
    }

    #[test]
    fn scan_period_cached_for_cyclic_only() {
        use core::time::Duration;
        let mut exec = Executor::builder().worker_threads(0).build().unwrap();
        let cyclic = exec
            .add(crate::item::item_with_triggers(
                |d| {
                    d.interval(Duration::from_millis(5));
                    Ok(())
                },
                |_| Ok(crate::ControlFlow::Continue),
            ))
            .unwrap();
        let event_driven = exec.add(item(|_| Ok(ControlFlow::Continue))).unwrap();

        let cyclic_entry = exec
            .tasks
            .iter()
            .find(|t| t.id == cyclic)
            .expect("cyclic task present");
        assert_eq!(cyclic_entry.scan_period, Some(Duration::from_millis(5)));
        // Sentinel: no sample has been taken yet.
        assert_eq!(cyclic_entry.last_took_ns.load(Ordering::Relaxed), u64::MAX);

        let event_entry = exec
            .tasks
            .iter()
            .find(|t| t.id == event_driven)
            .expect("event-driven task present");
        assert_eq!(event_entry.scan_period, None);
    }

    #[test]
    fn cycle_stats_index_aligned_with_tasks() {
        use core::time::Duration;
        let mut exec = Executor::builder()
            .worker_threads(0)
            .stats_window(512)
            .build()
            .unwrap();
        // Builder option flows through to the executor.
        assert_eq!(exec.stats_window, 512);
        // No tasks yet → both Vecs empty and aligned.
        assert_eq!(exec.cycle_stats.len(), exec.tasks.len());

        // Cyclic single-item add path.
        exec.add(crate::item::item_with_triggers(
            |d| {
                d.interval(Duration::from_millis(5));
                Ok(())
            },
            |_| Ok(crate::ControlFlow::Continue),
        ))
        .unwrap();
        // Event-driven single-item add path.
        exec.add(item(|_| Ok(ControlFlow::Continue))).unwrap();

        assert_eq!(exec.tasks.len(), 2);
        assert_eq!(exec.cycle_stats.len(), exec.tasks.len());
    }

    #[test]
    fn add_with_fault_handler_stores_handler_job() {
        use core::time::Duration;
        let mut exec = Executor::builder().worker_threads(0).build().unwrap();
        let task_id = exec
            .add_with_fault_handler(
                crate::item::item_with_triggers(
                    |d| {
                        d.interval(Duration::from_millis(10));
                        d.budget(Duration::from_millis(5));
                        Ok(())
                    },
                    |_| Ok(crate::ControlFlow::Continue),
                ),
                crate::item::item_with_triggers(|_d| Ok(()), |_| Ok(crate::ControlFlow::Continue)),
            )
            .unwrap();
        let entry = exec
            .tasks
            .iter()
            .find(|t| t.id == task_id)
            .expect("task present");
        assert!(
            entry.handler_job.is_some(),
            "handler_job should be Some after add_with_fault_handler"
        );
        // Main job should still be present.
        assert!(entry.job.is_some(), "main job should still be present");
    }

    #[test]
    fn declare_triggers_called_at_add_time() {
        let called = Arc::new(AtomicBool::new(false));
        let called_d = Arc::clone(&called);

        let it = crate::item::item_with_triggers(
            move |_d| {
                called_d.store(true, Ordering::SeqCst);
                Ok(())
            },
            |_| Ok(ControlFlow::Continue),
        );

        let mut exec = Executor::builder().worker_threads(0).build().unwrap();
        exec.add(it).unwrap();
        assert!(called.load(Ordering::SeqCst));
    }

    #[test]
    fn clear_task_fault_errors_on_running_task() {
        use core::time::Duration;
        let mut exec = Executor::builder().worker_threads(0).build().unwrap();
        let task_id = exec
            .add(crate::item::item_with_triggers(
                |d| {
                    d.interval(Duration::from_millis(10));
                    Ok(())
                },
                |_| Ok(crate::ControlFlow::Continue),
            ))
            .unwrap();
        // Task starts in Running state — clearing should error.
        let err = exec.clear_task_fault(task_id).expect_err("not faulted");
        assert!(matches!(err, ExecutorError::TaskNotFaulted(_)));
    }

    #[test]
    fn clear_executor_fault_errors_on_running_executor() {
        let exec = Executor::builder().worker_threads(0).build().unwrap();
        let err = exec.clear_executor_fault().expect_err("not faulted");
        assert!(matches!(err, ExecutorError::ExecutorNotFaulted));
    }

    #[test]
    fn overrun_count_returns_zero_for_new_task() {
        use core::time::Duration;
        let mut exec = Executor::builder().worker_threads(0).build().unwrap();
        let task_id = exec
            .add(crate::item::item_with_triggers(
                |d| {
                    d.interval(Duration::from_millis(10));
                    d.budget(Duration::from_millis(5));
                    Ok(())
                },
                |_| Ok(crate::ControlFlow::Continue),
            ))
            .unwrap();
        assert_eq!(exec.overrun_count(task_id).unwrap(), 0);
    }

    #[test]
    fn overrun_count_errors_for_unknown_task() {
        let exec = Executor::builder().worker_threads(0).build().unwrap();
        let err = exec
            .overrun_count(crate::TaskId::new("nope"))
            .expect_err("unknown task");
        assert!(matches!(err, ExecutorError::TaskNotFound(_)));
    }

    #[test]
    fn task_fault_state_starts_running() {
        use core::time::Duration;
        let mut exec = Executor::builder().worker_threads(0).build().unwrap();
        let task_id = exec
            .add(crate::item::item_with_triggers(
                |d| {
                    d.interval(Duration::from_millis(10));
                    Ok(())
                },
                |_| Ok(crate::ControlFlow::Continue),
            ))
            .unwrap();
        assert_eq!(exec.task_fault_state(task_id).unwrap(), FaultState::Running);
    }

    #[test]
    fn executor_fault_state_starts_running() {
        let exec = Executor::builder().worker_threads(0).build().unwrap();
        assert_eq!(exec.executor_fault_state(), ExecutorFaultState::Running);
    }

    // --- on_fatal / FatalDispatch integration tests ---

    #[test]
    fn build_without_on_fatal_succeeds() {
        use crate::fatal::{FatalContext, FatalSite};
        use std::sync::{Arc, Mutex};
        // Default builder (no on_fatal) must build successfully.
        let exec = Executor::builder().worker_threads(0).build().unwrap();
        // The fatal_dispatch field is present; fire via a test terminal to
        // confirm the no-op handler doesn't blow up.
        let reached: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
        let reached2 = Arc::clone(&reached);
        let test_dispatch = crate::fatal::FatalDispatch::with_terminal(
            exec.fatal_dispatch.handler().clone(),
            move |_| {
                *reached2.lock().unwrap() = true;
            },
        );
        test_dispatch.fire(&FatalContext {
            cause: "test".to_string(),
            site: FatalSite::PoolWorker,
        });
        assert!(*reached.lock().unwrap(), "terminal not reached");
    }

    #[test]
    fn on_fatal_handler_is_stored_and_invoked() {
        use crate::fatal::{FatalContext, FatalSite};
        use std::sync::{Arc, Mutex};
        let called: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let called2 = Arc::clone(&called);
        let exec = Executor::builder()
            .worker_threads(0)
            .on_fatal(move |ctx| {
                called2.lock().unwrap().push(ctx.cause.clone());
            })
            .build()
            .unwrap();
        // Verify the handler fires via a test terminal.
        let reached: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
        let reached2 = Arc::clone(&reached);
        let test_dispatch = crate::fatal::FatalDispatch::with_terminal(
            exec.fatal_dispatch.handler().clone(),
            move |_| {
                *reached2.lock().unwrap() = true;
            },
        );
        test_dispatch.fire(&FatalContext {
            cause: "my-cause".to_string(),
            site: FatalSite::ExecutorRunLoop,
        });
        assert!(*reached.lock().unwrap(), "terminal not reached");
        let log = called.lock().unwrap().clone();
        assert_eq!(
            log,
            vec!["my-cause"],
            "handler should have been called with cause"
        );
    }
}
