//! Parallel-execution graph: a DAG of [`ExecutableItem`]s rooted at a single
//! vertex whose triggers gate the whole graph.

use crate::error::ExecutorError;
use crate::item::ExecutableItem;
use crate::trigger::{TriggerDecl, TriggerDeclarer};
use taktora_executor_sys::dispatch::ExclusiveCell;

/// Opaque handle to a graph vertex. Returned by [`GraphBuilder::vertex`].
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct Vertex(pub(crate) usize);

/// Internal graph storage.
///
/// Stored inside `TaskKind::Graph(Box<Graph>)` to guarantee a stable heap
/// address — the per-vertex dispatch closures capture a `*const Graph`
/// pointing back into this struct, and would dangle if the `Graph` moved.
/// All runtime state below is pre-allocated at `finish()` time and reset
/// in place each `run_once_borrowed` call. Required for `REQ_0060`.
#[allow(clippy::redundant_pub_crate)]
pub(crate) struct Graph {
    pub(crate) successors: Vec<Vec<usize>>, // adjacency list
    pub(crate) in_degree: Vec<usize>,       // initial in-degree
    pub(crate) root: usize,
    pub(crate) decls: Vec<TriggerDecl>,

    // ── Pre-allocated dispatch state (REQ_0060) ────────────────────────
    /// Shared items wrapped in `Arc<ExclusiveCell<...>>` for zero-alloc
    /// dispatch. Populated once in `finish`. Each vertex closure captures
    /// an Arc clone (refcount only, no alloc per run).
    pub(crate) items_shared: Vec<std::sync::Arc<ExclusiveCell<Box<dyn ExecutableItem>>>>,
    /// Per-vertex integrity levels, captured at assembly (before the items
    /// are shared) so setup-time checks need no cell access.
    pub(crate) integrity_levels: Vec<crate::IntegrityLevel>,
    /// The root vertex's `task_id()` override, captured at assembly.
    root_task_id: Option<String>,
    /// Shared runtime state accessed by vertex closures (atomics, ready ring,
    /// etc.). Wrapped in Arc so closures can capture a clone (refcount only).
    shared: std::sync::Arc<GraphShared>,
    /// Per-vertex pre-built dispatch closures wrapped in `Arc<ExclusiveCell<...>>`.
    /// Empty after `finish`, populated by `prepare_dispatch` when the graph
    /// is registered with an executor. Used by `run_once_borrowed` via
    /// `Pool::submit_shared`, avoiding per-vertex allocation (Arc clone is
    /// refcount only). Required for `REQ_0060`.
    vertex_jobs: Vec<std::sync::Arc<ExclusiveCell<dyn FnMut() + Send>>>,
}

impl core::fmt::Debug for Graph {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Graph")
            .field("n_items", &self.items_shared.len())
            .field("successors", &self.successors)
            .field("in_degree", &self.in_degree)
            .field("root", &self.root)
            .finish_non_exhaustive()
    }
}

impl Graph {
    /// Return the root vertex's `task_id()` override, if any.
    pub(crate) fn root_task_id(&self) -> Option<&str> {
        self.root_task_id.as_deref()
    }
}

/// Builder for a graph.
pub struct GraphBuilder {
    items: Vec<Box<dyn ExecutableItem>>,
    edges: Vec<(usize, usize)>,
    root: Option<usize>,
}

impl GraphBuilder {
    pub(crate) fn new() -> Self {
        Self {
            items: Vec::new(),
            edges: Vec::new(),
            root: None,
        }
    }

    /// Add a vertex; returns its handle.
    pub fn vertex<I: ExecutableItem>(&mut self, item: I) -> Vertex {
        let idx = self.items.len();
        self.items.push(Box::new(item));
        Vertex(idx)
    }

    /// Add a directed edge `from -> to`.
    pub fn edge(&mut self, from: Vertex, to: Vertex) -> &mut Self {
        self.edges.push((from.0, to.0));
        self
    }

    /// Designate the root vertex (whose triggers gate the graph).
    pub const fn root(&mut self, v: Vertex) -> &mut Self {
        self.root = Some(v.0);
        self
    }

    /// Build, validating connectedness, acyclicity, and exactly-one root.
    ///
    /// The validation steps run in the same precedence as before — non-empty,
    /// root presence/bounds, edge validity, acyclicity, reachability, then
    /// trigger collection — each delegated to a private helper that surfaces
    /// the first failing condition via `?`. Behaviour is identical to the
    /// previous inline form: the earliest violating check still wins.
    pub(crate) fn finish(mut self) -> Result<Graph, ExecutorError> {
        let n = self.items.len();
        let root = Self::validate_root(self.root, n)?;
        let (successors, in_degree) = Self::build_adjacency(&self.edges, n)?;
        Self::assert_acyclic(&successors, &in_degree, n)?;
        Self::assert_reachable(&successors, root, n)?;
        let decls = self.collect_root_decls(root)?;
        Ok(Self::assemble(
            self.items, successors, in_degree, root, decls,
        ))
    }

    /// Validate that the graph is non-empty and that a root vertex was set and
    /// is in bounds. Returns the validated root index.
    ///
    /// Preserves the original precedence: empty-graph rejection wins over the
    /// missing-root check, which wins over the out-of-bounds check.
    fn validate_root(root: Option<usize>, n: usize) -> Result<usize, ExecutorError> {
        if n == 0 {
            return Err(ExecutorError::InvalidGraph("graph has no vertices".into()));
        }
        let root = root.ok_or_else(|| ExecutorError::InvalidGraph("no root vertex set".into()))?;
        if root >= n {
            return Err(ExecutorError::InvalidGraph(
                "root index out of bounds".into(),
            ));
        }
        Ok(root)
    }

    /// Build the adjacency list and per-vertex initial in-degree from `edges`,
    /// rejecting out-of-bounds endpoints and self-loops (in that precedence,
    /// matching the original inline loop).
    fn build_adjacency(
        edges: &[(usize, usize)],
        n: usize,
    ) -> Result<(Vec<Vec<usize>>, Vec<usize>), ExecutorError> {
        let mut successors = vec![Vec::<usize>::new(); n];
        let mut in_degree = vec![0_usize; n];
        for &(from, to) in edges {
            if from >= n || to >= n {
                return Err(ExecutorError::InvalidGraph(
                    "edge index out of bounds".into(),
                ));
            }
            if from == to {
                return Err(ExecutorError::InvalidGraph(
                    "self-loops are not allowed".into(),
                ));
            }
            successors[from].push(to);
            in_degree[to] += 1;
        }
        Ok((successors, in_degree))
    }

    /// Reject graphs that contain a cycle, via Kahn's algorithm. `in_degree`
    /// is cloned internally because the algorithm mutates it.
    fn assert_acyclic(
        successors: &[Vec<usize>],
        in_degree: &[usize],
        n: usize,
    ) -> Result<(), ExecutorError> {
        let mut k_in = in_degree.to_vec();
        let mut queue: Vec<usize> = k_in
            .iter()
            .enumerate()
            .filter_map(|(i, d)| (*d == 0).then_some(i))
            .collect();
        let mut visited = 0_usize;
        while let Some(u) = queue.pop() {
            visited += 1;
            for &v in &successors[u] {
                k_in[v] -= 1;
                if k_in[v] == 0 {
                    queue.push(v);
                }
            }
        }
        if visited != n {
            return Err(ExecutorError::InvalidGraph("graph contains a cycle".into()));
        }
        Ok(())
    }

    /// Reject graphs in which some vertex is unreachable from `root`, via DFS.
    fn assert_reachable(
        successors: &[Vec<usize>],
        root: usize,
        n: usize,
    ) -> Result<(), ExecutorError> {
        let mut reach = vec![false; n];
        let mut stack = vec![root];
        while let Some(u) = stack.pop() {
            if reach[u] {
                continue;
            }
            reach[u] = true;
            for &v in &successors[u] {
                stack.push(v);
            }
        }
        if reach.iter().any(|r| !*r) {
            return Err(ExecutorError::InvalidGraph(
                "every vertex must be reachable from the root".into(),
            ));
        }
        Ok(())
    }

    /// Collect the root vertex's trigger declarations (which gate the whole
    /// graph) and warn about any triggers declared by non-root vertices, which
    /// are ignored. Propagates a declaration error from the root vertex.
    fn collect_root_decls(&mut self, root: usize) -> Result<Vec<TriggerDecl>, ExecutorError> {
        let mut decl = TriggerDeclarer::new_internal();
        self.items[root].declare_triggers(&mut decl)?;
        let decls = decl.into_decls();

        for (i, body) in self.items.iter_mut().enumerate() {
            if i == root {
                continue;
            }
            let mut spurious = TriggerDeclarer::new_internal();
            let _ = body.declare_triggers(&mut spurious);
            if !spurious.is_empty() {
                #[cfg(feature = "tracing")]
                tracing::warn!(target: "taktora-executor", vertex = i,
                    "non-root graph vertex declared triggers; ignored");
            }
        }
        Ok(decls)
    }

    /// Assemble the validated pieces into a `Graph`, pre-allocating the runtime
    /// dispatch state (`REQ_0060`).
    fn assemble(
        items: Vec<Box<dyn ExecutableItem>>,
        successors: Vec<Vec<usize>>,
        in_degree: Vec<usize>,
        root: usize,
        decls: Vec<TriggerDecl>,
    ) -> Graph {
        let n_items = items.len();
        let integrity_levels = items
            .iter()
            .map(|it| ExecutableItem::integrity_level(it.as_ref()))
            .collect::<Vec<_>>();
        let root_task_id = items[root].task_id().map(str::to_string);
        // Wrap each item in Arc<ExclusiveCell<...>>. This is the single
        // ownership location for items; all accesses (build-time and dispatch)
        // go through ExclusiveCell::try_with. Required for `REQ_0060`.
        let items_shared: Vec<std::sync::Arc<ExclusiveCell<Box<dyn ExecutableItem>>>> = items
            .into_iter()
            .map(|b| std::sync::Arc::new(ExclusiveCell::new(b)))
            .collect();
        let counters: Vec<AtomicUsize> = in_degree.iter().map(|d| AtomicUsize::new(*d)).collect();
        let shared = std::sync::Arc::new(GraphShared {
            counters,
            pending: AtomicUsize::new(n_items),
            stop_flag: AtomicBool::new(false),
            stop_chain_seen: AtomicBool::new(false),
            first_err: Mutex::new(None),
            done_cv: (Mutex::new(()), Condvar::new()),
            ready_ring: crate::ready_ring::ReadyRing::new(n_items),
            successors: successors.clone(),
        });

        Graph {
            successors,
            in_degree,
            root,
            decls,
            items_shared,
            integrity_levels,
            root_task_id,
            shared,
            vertex_jobs: Vec::new(),
        }
    }
}

// ── Graph scheduler (Task 14) ─────────────────────────────────────────────────

use crate::context::Stoppable;
use crate::monitor::ExecutionMonitor;
use crate::observer::Observer;
use crate::pool::Pool;
use crate::task_id::TaskId;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};

/// Outcome of running a graph once.
#[allow(clippy::redundant_pub_crate)]
pub(crate) struct GraphRunOutcome {
    #[allow(clippy::redundant_pub_crate)]
    pub(crate) error: Option<crate::error::ItemError>,
    #[allow(clippy::redundant_pub_crate)]
    pub(crate) stopped_chain: bool,
}

/// Shared graph state captured by vertex closures.
///
/// Contains only the fields accessed by vertex dispatch closures (atomics,
/// Mutex slots, ready ring, and the adjacency list). The per-vertex items
/// are captured separately as `Arc<ExclusiveCell<Box<dyn ExecutableItem>>>`.
struct GraphShared {
    counters: Vec<AtomicUsize>,
    pending: AtomicUsize,
    stop_flag: AtomicBool,
    stop_chain_seen: AtomicBool,
    first_err: Mutex<Option<crate::error::ItemError>>,
    done_cv: (Mutex<()>, Condvar),
    ready_ring: crate::ready_ring::ReadyRing,
    successors: Vec<Vec<usize>>,
}

impl GraphShared {
    #[deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    fn finalise_skipped(&self, i: usize) {
        if self.pending.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.notify_done();
            return;
        }
        for &j in &self.successors[i] {
            self.cancel_subtree(j);
        }
    }

    #[deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    fn cancel_subtree(&self, root: usize) {
        let mut stack = vec![root];
        while let Some(u) = stack.pop() {
            let prev = self.counters[u].swap(usize::MAX, Ordering::AcqRel);
            if prev != usize::MAX {
                if self.pending.fetch_sub(1, Ordering::AcqRel) == 1 {
                    self.notify_done();
                    return;
                }
                for &v in &self.successors[u] {
                    stack.push(v);
                }
            }
        }
    }

    #[deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    fn notify_done(&self) {
        #[allow(clippy::unwrap_used)]
        let _g = self.done_cv.0.lock().unwrap();
        self.done_cv.1.notify_all();
    }
}

impl Graph {
    /// Build per-vertex dispatch closures and stash them on the graph.
    /// Called once, when the graph is registered with an executor via
    /// `ExecutorGraphBuilder::build`. Each closure captures `Arc` clones
    /// (refcount-only at build time) and `Copy` primitives; no per-iteration
    /// allocation occurs in the resulting closures. Required for `REQ_0060`.
    #[deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    #[allow(clippy::too_many_lines, clippy::needless_pass_by_value)]
    pub(crate) fn prepare_dispatch(
        self: &mut Box<Self>,
        task_id: TaskId,
        stop: Stoppable,
        observer: Arc<dyn Observer>,
        monitor: Arc<dyn ExecutionMonitor>,
        err_slot: Arc<Mutex<Option<crate::error::ExecutorError>>>,
    ) {
        let n = self.items_shared.len();
        let shared = Arc::clone(&self.shared);

        let mut jobs: Vec<Arc<ExclusiveCell<dyn FnMut() + Send>>> = Vec::with_capacity(n);
        for i in 0..n {
            let task_id = task_id.clone();
            let stop = stop.clone();
            let observer = Arc::clone(&observer);
            let monitor = Arc::clone(&monitor);
            let err_slot = Arc::clone(&err_slot);
            let item_cell = Arc::clone(&self.items_shared[i]);
            let shared = Arc::clone(&shared);
            let successors_i = self.successors[i].clone();

            let job: Arc<ExclusiveCell<dyn FnMut() + Send>> =
                Arc::new(ExclusiveCell::new(Box::new(move || {
                    if shared.stop_flag.load(Ordering::Acquire) {
                        shared.finalise_skipped(i);
                        return;
                    }

                    let mut ctx = crate::context::Context::new(&task_id, &stop, observer.as_ref());

                    // Invariant: ExclusiveCell::try_with returns None if the item
                    // is busy. The graph's in-degree sequencing ensures at most one
                    // thread per vertex, so None is an invariant breach.
                    #[allow(clippy::expect_used)]
                    // Invariant: in-degree discipline ensures try_with succeeds
                    let (app_id, res) = item_cell
                        .try_with(|item| {
                            let app_id = item.app_id();
                            let app_inst = item.app_instance_id();
                            if let Some(aid) = app_id {
                                observer.on_app_start(task_id.clone(), aid, app_inst);
                            }
                            let started = std::time::Instant::now();
                            monitor.pre_execute(task_id.clone(), started);
                            let res = crate::executor::run_item_catch_unwind_external(
                                item.as_mut(),
                                &mut ctx,
                            );
                            let took = started.elapsed();
                            monitor.post_execute(task_id.clone(), started, took, res.is_ok());
                            (app_id, res)
                        })
                        .expect("ExclusiveCell busy: graph in-degree invariant breach");

                    if let Err(ref e) = res {
                        observer.on_app_error(task_id.clone(), e.as_ref());
                    }
                    if app_id.is_some() {
                        observer.on_app_stop(task_id.clone());
                    }

                    match &res {
                        Ok(crate::ItemFlow::Continue) => {}
                        Ok(crate::ItemFlow::StopChain) => {
                            shared.stop_chain_seen.store(true, Ordering::Release);
                            shared.stop_flag.store(true, Ordering::Release);
                        }
                        Err(_) => shared.stop_flag.store(true, Ordering::Release),
                    }

                    if let Err(e) = res {
                        // fail-fast: poison is unreachable — a holder panic aborts
                        // the process before any other thread observes the lock
                        // (ADR_0065)
                        #[allow(clippy::unwrap_used)]
                        let mut fe = shared.first_err.lock().unwrap();
                        if fe.is_none() {
                            *fe = Some(e);
                        }
                    }

                    if shared.pending.fetch_sub(1, Ordering::AcqRel) == 1 {
                        shared.notify_done();
                    } else if shared.stop_flag.load(Ordering::Acquire) {
                        for &j in &successors_i {
                            shared.cancel_subtree(j);
                        }
                    } else {
                        for &j in &successors_i {
                            if shared.counters[j].fetch_sub(1, Ordering::AcqRel) == 1 {
                                // fail-fast: ring is sized to next_power_of_two(n_vertices);
                                // each vertex becomes ready at most once per run, so overflow
                                // means broken in-degree accounting
                                #[allow(clippy::expect_used)]
                                shared
                                    .ready_ring
                                    .push(j)
                                    .expect("ready_ring sized to n_vertices");
                            }
                        }
                    }
                    let _ = &err_slot;
                })
                    as Box<dyn FnMut() + Send>));
            jobs.push(job);
        }
        self.vertex_jobs = jobs;
    }

    /// Dispatch this graph once and block until completion. Allocation-free
    /// in the steady state — runtime state was pre-allocated by
    /// `Graph::finish` and per-vertex closures by `prepare_dispatch`.
    /// Required by `REQ_0060`.
    // &mut enforces single-threaded access (contract), though atomics mean no direct mutation
    #[allow(clippy::needless_pass_by_ref_mut)]
    #[deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    pub(crate) fn run_once_borrowed(&mut self, pool: &Pool) -> GraphRunOutcome {
        let n = self.items_shared.len();

        // Reset per-iteration state in place.
        for (c, d) in self.shared.counters.iter().zip(self.in_degree.iter()) {
            c.store(*d, Ordering::Relaxed);
        }
        self.shared.pending.store(n, Ordering::Relaxed);
        self.shared.stop_flag.store(false, Ordering::Relaxed);
        self.shared.stop_chain_seen.store(false, Ordering::Relaxed);
        // fail-fast: poison is unreachable — a holder panic aborts the process
        // before any other thread observes the lock (ADR_0065)
        #[allow(clippy::unwrap_used)]
        {
            *self.shared.first_err.lock().unwrap() = None;
        }
        self.shared.ready_ring.reset();

        // Seed: dispatch every initially-ready vertex (those whose
        // **initial** in-degree is zero). Race-free — `in_degree` is
        // built once at finish() and never mutated, so we can't be
        // tricked by a worker that has already started running root
        // and decremented `counters[succ]` to zero before the seed
        // loop reaches `succ`. Reading `counters[i]` here would race
        // with the worker, redispatching the successor a second time
        // (the worker's own push to `ready_ring` is the legitimate
        // dispatch path).
        for i in 0..n {
            if self.in_degree[i] == 0 {
                self.dispatch_vertex(pool, i);
            }
        }

        // Drain ready_ring until pending hits 0.
        loop {
            while let Some(i) = self.shared.ready_ring.pop() {
                self.dispatch_vertex(pool, i);
            }
            if self.shared.pending.load(Ordering::Acquire) == 0 {
                break;
            }
            // fail-fast: poison is unreachable — a holder panic aborts the
            // process before any other thread observes the lock (ADR_0065)
            #[allow(clippy::unwrap_used)]
            let guard = self.shared.done_cv.0.lock().unwrap();
            if self.shared.pending.load(Ordering::Acquire) == 0 {
                drop(guard);
                break;
            }
            // fail-fast: condvar poison is unreachable under the abort
            // boundary (ADR_0065)
            #[allow(clippy::unwrap_used)]
            drop(
                self.shared
                    .done_cv
                    .1
                    .wait_timeout(guard, std::time::Duration::from_millis(5))
                    .unwrap()
                    .0,
            );
        }
        // Final drain.
        while self.shared.ready_ring.pop().is_some() {}

        // fail-fast: poison is unreachable — a holder panic aborts the process
        // before any other thread observes the lock (ADR_0065)
        #[allow(clippy::unwrap_used)]
        let mut first_err = self.shared.first_err.lock().unwrap();
        GraphRunOutcome {
            error: first_err.take(),
            stopped_chain: self.shared.stop_chain_seen.load(Ordering::Acquire),
        }
    }

    /// Submit vertex `i`'s pre-built closure to the pool. Allocation-free
    /// (uses `Pool::submit_shared`).
    #[deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    fn dispatch_vertex(&self, pool: &Pool, i: usize) {
        pool.submit_shared(&self.vertex_jobs[i]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ItemFlow, item};

    #[test]
    fn empty_graph_rejected() {
        let b = GraphBuilder::new();
        let err = b.finish().expect_err("empty graph");
        assert!(format!("{err}").contains("no vertices"));
    }

    #[test]
    fn missing_root_rejected() {
        let mut b = GraphBuilder::new();
        b.vertex(item(|_| Ok(ItemFlow::Continue)));
        let err = b.finish().expect_err("missing root");
        assert!(format!("{err}").contains("no root"));
    }

    #[test]
    fn cycle_rejected() {
        let mut b = GraphBuilder::new();
        let a = b.vertex(item(|_| Ok(ItemFlow::Continue)));
        let v = b.vertex(item(|_| Ok(ItemFlow::Continue)));
        b.edge(a, v).edge(v, a).root(a);
        let err = b.finish().expect_err("cycle");
        assert!(format!("{err}").contains("cycle"));
    }

    #[test]
    fn unreachable_vertex_rejected() {
        let mut b = GraphBuilder::new();
        let a = b.vertex(item(|_| Ok(ItemFlow::Continue)));
        let _orphan = b.vertex(item(|_| Ok(ItemFlow::Continue)));
        b.root(a);
        let err = b.finish().expect_err("unreachable");
        assert!(format!("{err}").contains("reachable"));
    }

    #[test]
    #[allow(clippy::many_single_char_names)]
    fn diamond_graph_builds() {
        let mut b = GraphBuilder::new();
        let r = b.vertex(item(|_| Ok(ItemFlow::Continue)));
        let l = b.vertex(item(|_| Ok(ItemFlow::Continue)));
        let rt = b.vertex(item(|_| Ok(ItemFlow::Continue)));
        let m = b.vertex(item(|_| Ok(ItemFlow::Continue)));
        b.edge(r, l).edge(r, rt).edge(l, m).edge(rt, m).root(r);
        let g = b.finish().expect("diamond");
        assert_eq!(g.successors[r.0], vec![l.0, rt.0]);
        assert_eq!(g.in_degree[m.0], 2);
    }
}
