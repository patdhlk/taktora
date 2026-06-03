//! `Executor` and `ExecutorBuilder`. Run loop lives in Task 8.

// Fields consumed by the run loop (Task 8) and graph scheduler (Task 14).
#![allow(dead_code)]
// pub(crate) inside a private module — intentional, Task 8+ will use them.
#![allow(clippy::redundant_pub_crate)]

use crate::Channel;
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

        for (task_idx, task) in self.tasks.iter().enumerate() {
            for decl in &task.decls {
                let guard = attach_trigger_decl(&waitset, &mut listener_storage, decl)?;
                guards.push(guard);
                attachment_to_task.push(task_idx);
            }
        }

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
                        exec_fault_ptr,
                        exec_start_ptr,
                        stop_listener_ptr,
                        pool,
                        iter_err: &iter_err_inner,
                    };

                    waitset.wait_and_process_once(
                        |attachment_id: WaitSetAttachmentId<ipc::Service>| {
                            pass.process_attachment(&attachment_id)
                        },
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
    /// request, then the active [`RunMode`] limit.
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

        // iceoryx2's WaitSet catches SIGINT/SIGTERM internally; honor that
        // here for a clean exit.
        if matches!(
            cb_result,
            WaitSetRunResult::Interrupt | WaitSetRunResult::TerminationRequest
        ) {
            return IterOutcome::Done;
        }

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

        iterations_done.fetch_add(1, Ordering::SeqCst);
        let reached_limit = match mode {
            RunMode::Forever => false,
            RunMode::Iterations(n) => iterations_done.load(Ordering::SeqCst) >= *n,
            RunMode::Until(deadline) => Instant::now() >= *deadline,
            RunMode::Predicate(p) => (p)(),
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
    /// Raw pointer to `Executor::exec_fault` inner state.
    exec_fault_ptr: *const ExecutorFaultAtomic,
    /// Raw pointer to `Executor::start_time`.
    exec_start_ptr: *const OnceLock<Instant>,
    /// Raw pointer to the internal stop listener.
    stop_listener_ptr: *const IxListener<ipc::Service>,
    /// Borrow of the executor thread pool.
    pool: &'a Pool,
    /// Refcount-only handle to the per-iteration error slot.
    iter_err: &'a Arc<std::sync::Mutex<Option<ExecutorError>>>,
}

impl DispatchPass<'_, '_, '_> {
    /// Handles a single `WaitSet` wakeup: drains stop notifications, then
    /// dispatches every task whose attachment fired. Always returns
    /// [`CallbackProgression::Continue`]; termination is decided by the
    /// `stop_flag` check in `dispatch_loop` after the callback returns.
    #[deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    #[allow(unsafe_code)]
    fn process_attachment(
        &mut self,
        attachment_id: &WaitSetAttachmentId<ipc::Service>,
    ) -> CallbackProgression {
        // Drain stop notifications first (no dispatch — the stop_flag check
        // after the callback returns handles termination).
        // SAFETY: stop_listener_ptr is valid for the duration of the call;
        // the Arc in self.stop_listener keeps it alive.
        let stop_l = unsafe { &*self.stop_listener_ptr };
        while let Ok(Some(_)) = stop_l.try_wait_one() {}

        for (i, guard) in self.guards.iter().enumerate() {
            let fired =
                attachment_id.has_event_from(guard) || attachment_id.has_missed_deadline(guard);
            if !fired {
                continue;
            }
            let task_idx = self.attachment_to_task[i];

            // SAFETY: we are the only thread that may touch the task table
            // during the callback. wait_and_process_once is single-threaded
            // and dispatch_loop holds &mut self. The pointer is valid for the
            // duration of this call.
            let task = unsafe { &mut (&mut *self.tasks_ptr)[task_idx] };

            // Pre-dispatch fault check (REQ_0070, REQ_0071, REQ_0072). When it
            // routes to a (possible) handler, normal dispatch is skipped.
            if self.handle_fault_routing(task) {
                continue;
            }

            self.dispatch_task(task);
        }

        // Wait for all submitted jobs to finish before leaving the callback
        // scope (validates item_ptr safety contract).
        self.pool.barrier();
        CallbackProgression::Continue
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
    fn dispatch_task(&self, task: &mut TaskEntry) {
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
) -> Box<dyn FnMut() + Send + 'static> {
    Box::new(move || {
        let mut ctx = crate::context::Context::new(&id, &stop, obs.as_ref());
        if let Some(aid) = app_id {
            obs.on_app_start(id.clone(), aid, app_inst);
        }
        let raw = item_ptr.get();
        let started = std::time::Instant::now();
        mon.pre_execute(id.clone(), started);
        // SAFETY: barrier() pairs with this invocation; the WaitSet
        // thread does not touch the item between `submit_borrowed` and
        // the matching `barrier()`. See SendItemPtr safety doc.
        #[allow(unsafe_code)]
        let res = run_item_catch_unwind(unsafe { &mut *raw }, &mut ctx);
        let took = started.elapsed();
        last_took_ns.store(
            u64::try_from(took.as_nanos()).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
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
) -> Box<dyn FnMut() + Send + 'static> {
    Box::new(move || {
        let mut ctx = crate::context::Context::new(&id, &stop, obs.as_ref());
        // Overall chain scan timer — the chain's `took` is the wall time
        // from the first item's pre-execute to the last item's completion
        // (or early break), mirroring the single-item `took` notion.
        let chain_started = std::time::Instant::now();
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
        let chain_took = chain_started.elapsed();
        last_took_ns.store(
            u64::try_from(chain_took.as_nanos()).unwrap_or(u64::MAX),
            Ordering::Relaxed,
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
        });
        self.executor
            .cycle_stats
            .push(TaskCycleStats::new(self.executor.stats_window));
        Ok(id)
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ControlFlow, item};

    #[test]
    fn add_returns_unique_ids() {
        let mut exec = Executor::builder().worker_threads(0).build().unwrap();
        let a = exec.add(item(|_| Ok(ControlFlow::Continue))).unwrap();
        let b = exec.add(item(|_| Ok(ControlFlow::Continue))).unwrap();
        assert_ne!(a, b);
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
