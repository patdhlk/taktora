Implementation
==============

The concrete Rust changes that realise the pre-allocated dispatch scratch
(:need:`BB_0023`) and deliver the zero-allocation steady-state dispatch of
:need:`REQ_0060`.

.. impl:: Zero-alloc dispatch — executor.rs + graph.rs refactor
   :id: IMPL_0001
   :status: implemented
   :implements: BB_0023
   :refines: REQ_0060

   Concrete Rust changes that realise :need:`BB_0023`.

   **In ``crates/taktora-executor/src/executor.rs``**

   * Add ``iter_err: Arc<Mutex<Option<ExecutorError>>>`` field on
     ``Executor`` (built once in ``Executor::build``). In
     ``dispatch_loop``, reset to ``None`` at the top of each
     iteration via ``*self.iter_err.lock().unwrap() = None``.
   * Add ``job: Option<Box<dyn FnMut() + Send + 'static>>``
     field on ``TaskEntry``. At ``add`` / ``add_chain`` time
     build the dispatch closure once with stable captures
     (``id``, ``stop``, ``Arc::clone`` of ``observer`` /
     ``monitor`` / ``iter_err``, raw ``SendItemPtr`` or
     ``SendChainPtr``) and store it on the task.
   * In ``dispatch_loop`` the ``Single`` and ``Chain`` arms
     dispatch via ``pool.submit_borrowed(BorrowedJob::new(task
     .job.as_deref_mut().unwrap() as *mut _))`` — no per-iter
     ``Box::new`` allocation.

   **In ``crates/taktora-executor/src/pool.rs``**

   * Generalise the worker job type from ``Box<dyn FnOnce>`` to
     an enum ``Job { Owned(Box<dyn FnOnce>), Borrowed(BorrowedJob)
     }`` so workers can run both styles.
   * Add ``unsafe fn submit_borrowed(&self, BorrowedJob)`` — the
     caller-owned closure path that performs no per-call
     allocation.

   **In ``crates/taktora-executor/src/graph.rs``**

   * Move ``counters``, ``pending``, ``stop_flag``,
     ``stop_chain_seen``, ``first_err``, ``done_cv``,
     ``vertex_ptrs``, and the ready ring from the per-call
     ``Arc<GraphRuntime>`` onto ``Graph`` itself. Reset (don't
     re-allocate) at the top of ``Graph::run_once_borrowed``.
   * Use ``&self.successors`` directly inside per-vertex
     closures via a ``SendGraphPtr`` (a ``*const Graph``
     wrapped in an ``unsafe Send + Sync`` marker).
   * Replace the per-call
     ``crossbeam_channel::unbounded::<usize>()`` with the
     ``ReadyRing`` defined in the new ``ready_ring`` module,
     stored as ``Graph::ready_ring`` and sized at ``finish``
     from ``next_power_of_two(n_vertices.max(2))``.
   * Pre-build one ``Box<dyn FnMut() + Send + 'static>`` per
     vertex in ``Graph::prepare_dispatch``, called by
     ``ExecutorGraphBuilder::build`` once the graph has been
     boxed and stable captures (task_id, stop, observer,
     monitor, err_slot) are known. Closures capture
     ``SendGraphPtr`` plus the per-vertex index.
   * In ``dispatch_loop`` the ``Graph`` arm calls
     ``graph.run_once_borrowed(pool)``; the graph dispatches
     each ready vertex via ``pool.submit_borrowed`` of its
     pre-built closure — no per-vertex ``Box`` per iter.
   * **Seed-loop race fix**: the seed dispatch in
     ``run_once_borrowed`` reads ``self.in_degree[i]``, not
     ``self.counters[i]``, when deciding which vertices to
     dispatch initially. Reading the runtime counter would
     race with the just-dispatched root's worker — if root
     starts running fast enough to decrement
     ``counters[successor]`` to zero before the seed loop
     reaches ``successor``, the seed loop would re-dispatch
     ``successor`` a second time. The worker's own
     ``ready_ring.push`` is the legitimate dispatch path
     for non-root vertices. ``in_degree`` is set once at
     ``finish()`` and never mutated — safe to read in any
     ordering. (Caught by the diamond test under the
     ``submit_borrowed`` path, which dispatches faster than
     the old per-vertex ``Box``-allocating path and so
     exposed the race that had previously been hidden by
     ``Box::new`` latency.)

   **In ``crates/taktora-executor/src/task_kind.rs``**

   * ``TaskKind::Graph(Box<Graph>)`` — Graph must live at a
     stable heap address because per-vertex closures capture
     ``*const Graph``.

   **New module ``crates/taktora-executor/src/ready_ring.rs``**

   * ``pub(crate) struct ReadyRing { buf: Box<[AtomicUsize]>,
     mask: usize, head: AtomicUsize, tail: AtomicUsize }``
     where ``usize::MAX`` is the empty sentinel.
   * ``new(min_capacity) -> Self`` rounds up to the next power
     of two (≥ 2) and pre-fills with the sentinel. One-time
     allocation.
   * ``reset(&self)``, ``push(&self, v) -> Result<(), ()>``,
     ``pop(&self) -> Option<usize>``. Producer side uses
     ``compare_exchange`` on ``tail`` (MPSC); consumer side
     spins briefly on the sentinel value when a slot has been
     reserved but the producer's value-store has not yet
     landed. Allocation-free in steady state.

   **Verification harness**

   * ``crates/taktora-executor/tests/no_alloc_dispatch.rs`` ships
     a hand-rolled counting ``#[global_allocator]`` (no new
     workspace dependency — covers pool worker threads, which
     ``assert_no_alloc``'s thread-local model does not).
     Differential measurement: ``per_iter = (run_n(100) -
     run_n(10)) / (100 - 10)`` separates setup-phase
     allocations from steady-state allocations. See
     :need:`TEST_0170`.
