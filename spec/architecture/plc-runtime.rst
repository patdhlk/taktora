PLC runtime — architecture
==========================

Detailed-design notes for the soft-real-time PLC heart family
(:need:`FEAT_0010`). This page currently covers the **bounded-time
dispatch** sub-feature (:need:`FEAT_0017`) and its zero-allocation
guarantee (:need:`REQ_0060`); other sub-features are added as their
designs land.

Per the arc42 conventions used across this spec, design decisions are
captured as ``arch-decision`` directives, structural elements as
``building-block`` directives, and concrete code mappings as ``impl``
directives. Test cases live in :doc:`../verification/plc-runtime`.

.. contents:: Sections
   :local:
   :depth: 1

----

Solution strategy
-----------------

The dispatch hot path's zero-allocation goal is solved by **moving every
per-iteration allocation up to ``Executor::build`` time** and reusing
that capacity. Two design choices follow from that posture: how to
reuse the per-iteration error slot, and how to replace the unbounded
crossbeam re-dispatch channel that ``Graph::run_once`` allocates today.

.. arch-decision:: Pre-allocate dispatch scratch at Executor::build time
   :id: ADR_0011
   :status: open
   :refines: REQ_0060

   **Context.** Today ``Executor::dispatch_loop`` allocates
   ``Arc<Mutex<Option<ExecutorError>>>`` on every iteration
   (``executor.rs:557-558``) and ``Graph::run_once`` allocates a
   fresh ``Vec<AtomicUsize>`` counter table, a fresh
   ``Arc<GraphRuntime>``, and a fresh
   ``crossbeam_channel::unbounded::<usize>()`` on every dispatch
   (``graph.rs:276-302``). None of those shapes change between
   iterations — vertex count, successor map, and error-channel
   width are fixed once ``Executor::build`` returns.

   **Decision.** Provision all per-iteration scratch at
   ``Executor::build`` time and reset (rather than reallocate) it
   on each tick of the dispatch loop. Concretely: hoist the
   error-capture slot onto ``Executor``, hoist the runtime
   counters / pending counter / successor borrow onto ``Graph``,
   and replace the unbounded re-dispatch channel with a
   hand-rolled bounded SPSC ring whose capacity is
   ``next_power_of_two(n_vertices)`` (see :need:`BB_0023`).

   **Alternatives considered.**

   * *Slab/arena per iteration.* Trades unconditional allocation
     for a slab reset, but slabs still allocate on resize and
     hide cost in the slab implementation. Rejected — the shapes
     are statically known, so a typed pre-allocation is sharper.
   * *Switch to ``smallvec`` everywhere.* Inline storage avoids
     small allocations but spills to the heap on overflow, which
     is non-deterministic — incompatible with a soft-real-time
     guarantee.
   * *Keep ``crossbeam_channel`` but call ``bounded(n)`` once.*
     Bounded crossbeam channels still allocate Arc'd shared state
     at construction, which is acceptable at build time but adds
     an external dependency we do not need on the hot path. A
     hand-rolled SPSC ring is a few dozen lines and removes the
     send-side allocation question entirely.

   **Consequences.**

   ✅ Steady-state dispatch performs zero heap allocations
   (per :need:`REQ_0060`).
   ✅ Worst-case re-dispatch latency is bounded by ring capacity,
   not allocator behaviour.
   ❌ Adds one ``unsafe`` block to ``taktora-executor`` (the SPSC
   ring push/pop), justified by a ``// SAFETY:`` comment and
   covered by ``loom`` tests under feature flag.
   ❌ Vertex count is now an explicit ``Executor::build`` input —
   builders that add vertices after build must rebuild
   (already the case in practice; documented explicitly).

----

Building blocks
---------------

.. building-block:: Dispatch scratch (pre-allocated)
   :id: BB_0023
   :status: open
   :implements: REQ_0060
   :refines: ADR_0011

   The collection of fields hoisted from per-iteration locals onto
   ``Executor`` and ``Graph`` so that dispatch reuses them. Three
   sub-components:

   * **iter_err slot** — single ``Mutex<Option<ExecutorError>>``
     stored on ``Executor``, reset to ``None`` at the start of
     each ``dispatch_loop`` iteration.
   * **Graph runtime fields** — ``counters: Vec<AtomicUsize>``,
     ``pending: AtomicUsize``, ``first_err: Mutex<Option<...>>``,
     ``stop_flag: AtomicBool``, ``stop_chain_seen: AtomicBool``,
     ``done_cv: (Mutex, Condvar)`` — all stored on ``Graph``,
     reset at the top of ``Graph::run_once``. ``self.successors``
     is borrowed rather than cloned.
   * **Re-dispatch SPSC ring** — bounded, ``Box<[AtomicUsize]>``
     of length ``next_power_of_two(n_vertices)``, owned by
     ``Graph``. Producer = pool worker; consumer = WaitSet
     thread. Used to communicate "vertex ``j`` became ready"
     from worker to scheduler without per-iteration allocation.

   Lifetime contract: every field is created in ``Executor::build``
   (or ``Graph::build`` when the executor builds its graphs) and
   lives for the lifetime of the ``Executor``. Reset semantics —
   not deallocation — drive per-iteration state hygiene.

Foundation building blocks (taktora-executor v0.1)
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

The following building blocks document the executor surfaces that
already exist in ``crates/taktora-executor`` and that the foundation
requirements (:need:`REQ_0001` .. :need:`REQ_0051`) refer to. They
exist as audit-trail targets so the foundation reqs can carry
``:status: implemented`` with a non-empty ``:links:`` to a realising
artefact.

.. building-block:: Cyclic scan trigger and dispatch
   :id: BB_0025
   :status: implemented
   :implements: FEAT_0011

   ``TriggerDeclarer::interval(period)``
   (``crates/taktora-executor/src/trigger.rs``) lets each registered
   item declare a scan period as a ``core::time::Duration``. The
   ``WaitSet`` attaches one interval guard per declaration
   (``executor.rs::WaitSet::attach_interval``) and the dispatch loop
   invokes the item's pre-built closure once per fire. The
   ``ExecutionMonitor`` trait (``src/monitor.rs``) brackets each
   invocation with ``pre_execute`` / ``post_execute`` callbacks so
   scan-cycle execution is observable.

.. building-block:: Iceoryx2 channel surface (Channel / Publisher / Subscriber)
   :id: BB_0026
   :status: implemented
   :implements: FEAT_0012

   ``Channel<T>`` plus the ``Publisher<T>`` / ``Subscriber<T>``
   primitives in ``crates/taktora-executor/src/channel.rs`` wrap an
   iceoryx2 publish-subscribe service. ``Publisher`` exposes three
   send paths — ``send_copy``, ``loan_send``, and ``loan`` — and
   returns ``NotifyOutcome { sent, listeners_notified }`` so the
   sender can detect dropped notifications without an error.
   ``Subscriber::take`` returns a borrowed view of the producer's
   payload (``iceoryx2::sample::Sample``), preserving the zero-copy
   IPC posture. ``TriggerDeclarer::subscriber(&sub)`` wires the
   subscriber's listener handle so a fresh sample wakes the item's
   dispatch.

.. building-block:: Chain and DAG sequencing primitives
   :id: BB_0027
   :status: implemented
   :implements: FEAT_0013

   ``Executor::add_chain`` builds a ``TaskKind::Chain`` whose
   pre-built closure iterates the items in declared order on a
   single pool slot per invocation. ``Executor::add_graph`` builds a
   ``TaskKind::Graph`` whose vertex closures decrement successor
   in-degree counters and dispatch ready successors via the
   ``ReadyRing``. Abort propagation: ``Ok(ControlFlow::StopChain)``
   or ``Err`` in a chain breaks the iterator
   (``executor.rs::build_chain_job``); in a graph the same outcomes
   flip a ``stop_flag`` and short-circuit the subtree
   (``graph.rs::cancel_subtree``). ``wrap_with_condition(item,
   predicate)`` in ``src/condition.rs`` gates execution on a
   runtime-evaluated predicate; a ``false`` predicate returns
   ``Ok(ControlFlow::StopChain)``.

.. building-block:: Deadline trigger and timing monitor
   :id: BB_0028
   :status: implemented
   :implements: FEAT_0014

   ``TriggerDeclarer::deadline(subscriber, deadline)``
   (``src/trigger.rs``) records a per-subscriber deadline; the
   ``WaitSet`` attaches a deadline guard (``attach_deadline``) and
   the dispatch callback honors ``has_missed_deadline`` when the
   guard fires. ``ExecutionMonitor::post_execute(task, started_at,
   took, ok)`` reports actual execute duration per invocation, so
   cycle-time overruns are observable from outside the executor.

.. building-block:: ThreadAttributes (worker affinity and priority)
   :id: BB_0029
   :status: implemented
   :implements: FEAT_0015

   ``ThreadAttributes`` in ``crates/taktora-executor/src/thread_attrs.rs``,
   gated behind the ``thread_attrs`` cargo feature, exposes
   ``name_prefix``, ``affinity_mask: Vec<usize>``, and ``priority:
   Option<i32>`` setters. ``Pool`` workers apply the attributes in
   their thread bodies: ``core_affinity::set_for_current`` pins the
   worker to the configured CPU set, and on ``target_os = "linux"``
   ``set_sched_fifo`` calls ``libc::pthread_setschedparam(...,
   SCHED_FIFO, ...)`` with the configured priority. Failure to apply
   ``SCHED_FIFO`` (typical on processes without ``CAP_SYS_NICE``) is
   silently tolerated — the worker keeps running under the default
   ``SCHED_OTHER``.

.. building-block:: Cooperative shutdown (Stoppable + WaitSet stop wakeup)
   :id: BB_0035
   :status: implemented
   :implements: FEAT_0016

   ``Stoppable`` (``crates/taktora-executor/src/context.rs``) is
   ``Clone`` and carries an iceoryx2 ``Notifier`` wired at
   ``Executor::build`` time. ``Stoppable::stop()`` flips an
   ``AtomicBool`` and calls ``notifier.notify()`` so the WaitSet
   wakes even when no other trigger is pending. The dispatch loop
   (``src/executor.rs``) further matches
   ``WaitSetRunResult::Interrupt`` and
   ``WaitSetRunResult::TerminationRequest`` and returns ``Ok(())``
   so SIGINT / SIGTERM exit the process cleanly.

----

Implementation
--------------

.. impl:: Zero-alloc dispatch — executor.rs + graph.rs refactor
   :id: IMPL_0001
   :status: open
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

----

Scan-cycle observability
------------------------

Detailed design for the **scan-cycle observability** sub-feature
(:need:`FEAT_0021`). Two structural pieces: a fixed-bucket histogram
for percentile estimation (chosen for its allocation-free, bounded-time
per-sample update path), and per-task aggregate slots allocated at
``Executor::build`` time.

.. arch-decision:: Fixed-bucket histogram for percentile estimation
   :id: ADR_0060
   :status: open
   :refines: REQ_0100

   **Context.** :need:`REQ_0100` requires p50 / p95 / p99
   execute-duration percentiles per task over a sliding window, and
   :need:`REQ_0104` requires the update path to be allocation-free with
   bounded per-sample latency. A window-of-raw-samples approach (keep
   the last N samples, sort on query) is allocation-free if N is fixed
   at build time but pays O(N log N) on every query. Streaming sketches
   (t-digest, CKMS) give tight p99 accuracy but their compaction step
   is amortised, not bounded, and they reshape memory as data arrives.

   **Decision.** Use a fixed-bucket log-linear histogram covering the
   value range 100 ns … 10 s with at least three buckets per decade
   (eight decades × three buckets ≈ 24 active buckets, padded to a
   power of two for cheap indexing). The bucket layout is fixed at
   compile time as a ``const`` table; the per-sample update is a
   ``log2``-style index computation plus an atomic increment.
   Percentile queries scan the bucket array in O(B) where B is
   constant (~32). Sliding-window behaviour is implemented as a small
   ring of histogram snapshots (size = window-count divided by
   snapshot period); ageing-out is a snapshot subtraction.

   **Alternatives considered.**

   * *Exact sliding window of raw samples.* Allocation-free if the
     ring is pre-allocated, but percentile query is O(N log N) and
     the ring must be sized for the worst case (~1 MB per task at
     100 k samples vs ~1 kB for the histogram). Rejected for memory
     pressure under many-task configurations.
   * *t-digest / CKMS streaming sketch.* Tighter p99 accuracy but
     compaction is amortised; worst-case per-sample latency is not
     bounded. Rejected because the per-sample update is on the
     dispatch hot path.

   **Consequences.**

   ✅ Per-sample update is O(1) and allocation-free
   (per :need:`REQ_0104`).
   ✅ Per-task memory footprint is bounded and known at build time
   (~1 kB / task for the histogram + snapshots).
   ❌ Percentile values are bucket-quantised — relative accuracy is
   bounded by bucket width (~33% within a single bucket, ≤ 1% at
   the bucket centroid). Acceptable for soft-RT telemetry; the
   :need:`REQ_0111` harness exposes raw samples for finer offline
   analysis when needed.

   **Amendment (:need:`REQ_0105`, :need:`REQ_0106`).** The histogram is
   retained as the percentile estimator, but two quantities are added
   alongside it because the histogram cannot supply them:

   * *Exact windowed min/max* (:need:`REQ_0105`). Snapshot subtraction
     ages out counts, not extrema — once the snapshot holding the
     worst-case sample is subtracted, the true maximum is unrecoverable
     from bucket counts. Exact windowed min/max therefore use a
     fixed-capacity **monotonic deque** (one for min, one for max), sized
     to the window at ``Executor::build`` time. Update and ageing are
     amortised O(1); memory is bounded by the window length.
   * *Deadline lateness* (:need:`REQ_0106`). A signed quantity (the task
     may start early or late) measured against the nominal periodic grid,
     distinct from the unsigned period jitter the histogram/max-jitter
     path already tracks. Its windowed maximum is held in an atomic field
     analogous to ``max_jitter_ns``.

   Both additions preserve the allocation-free, bounded-time per-sample
   update contract of :need:`REQ_0104`.

.. building-block:: Per-task cycle statistics
   :id: BB_0050
   :status: open
   :implements: REQ_0100, REQ_0105, REQ_0106
   :refines: ADR_0060

   ``CycleStats`` — per-task statistics owned by ``Executor``,
   allocated once at ``Executor::build`` time. Fields:

   * ``hist: Histogram`` — fixed-bucket histogram of execute durations
     per :need:`ADR_0060`.
   * ``min_max: MinMaxDeque`` — fixed-capacity monotonic deques holding
     the exact windowed min and max execute duration (per
     :need:`REQ_0105`); the histogram cannot recover an exact extremum
     after snapshot subtraction.
   * ``max_jitter_ns: AtomicU64`` — windowed maximum of
     ``|actual_period - declared_period|`` (per :need:`REQ_0101`).
   * ``max_lateness_ns: AtomicI64`` — windowed maximum signed deadline
     lateness against the nominal grid (per :need:`REQ_0106`); signed
     because a task may start early or late.
   * ``overrun_count: AtomicU64`` — monotonic counter, incremented when
     a scan-cycle exceeds the declared period (per :need:`REQ_0102`).

   The histogram, deques, and atomic fields are provided by the shared
   ``taktora-stats`` primitive (:need:`ADR_0062`), not reimplemented in
   the executor. One ``CycleStats`` per registered task; the array is
   sized at ``Executor::build``. Update paths use relaxed atomic stores
   so workers do not synchronise on the stats field.

.. building-block:: Statistics snapshot view
   :id: BB_0051
   :status: open
   :implements: REQ_0103, REQ_0105, REQ_0106
   :refines: ADR_0060

   ``StatsSnapshot`` — borrowed view returned by the pull API
   (``Executor::stats_snapshot``). Per-task entries carry
   ``{ task_id, p50_ns, p95_ns, p99_ns, min_ns, max_ns,
   max_jitter_ns, max_lateness_ns, overrun_count }`` computed from the
   matching :need:`BB_0050` at the moment of the call (``min_ns`` /
   ``max_ns`` per :need:`REQ_0105`, ``max_lateness_ns`` per
   :need:`REQ_0106`). The read is lossy-but-cheap: fields are loaded
   with relaxed atomics and may reflect samples taken microseconds
   apart, so the writer (dispatch loop) never blocks on a reader. The
   snapshot itself is a thin slice over pre-allocated buffers on
   ``Executor``; the caller may clone it for off-stack consumption but
   the runtime side never allocates.

.. impl:: Stats module — taktora-executor/src/stats/
   :id: IMPL_0070
   :status: open
   :implements: BB_0050, BB_0051
   :refines: REQ_0100

   Concrete Rust changes that realise :need:`BB_0050` and
   :need:`BB_0051`.

   **Shared primitive — ``taktora-stats`` crate** (per :need:`ADR_0062`)

   The allocation-free ``Histogram`` (fixed bucket table from
   :need:`ADR_0060`), the fixed-capacity ``MinMaxDeque``
   (:need:`REQ_0105`), and the atomic aggregate fields live in the
   ``no_std`` ``taktora-stats`` crate so the connector layer
   (:need:`ADR_0063`) reuses the same code. The executor depends on it;
   this supersedes the earlier plan to define the histogram inside
   ``taktora-executor``.

   **Module ``crates/taktora-executor/src/stats/`` (thin wrapper)**

   * ``mod.rs`` — defines the std-side value types only:
     ``CycleObservation { cycle_index, task_id, period_ns,
     actual_period_ns, jitter_ns, lateness_ns, took_ns }``,
     ``StatsSnapshot``, and ``TaskStatsEntry``.
     ``lateness_ns: i64`` is the signed deadline lateness of
     :need:`REQ_0106`; ``cycle_index`` is the monotonic per-task scan
     count and FEAT_0038 join key of :need:`REQ_0107`.
     There is no ``cycle.rs`` and no executor-side ``CycleStats`` struct.

   The per-task aggregator (``ExecutorCycleStats<S,W>``) lives in the
   ``no_std`` ``taktora-stats`` crate, mirroring ``ConnectorCycleStats``
   per :need:`ADR_0062`. It holds a ``CycleStatsCore`` (histogram +
   exact min/max) plus ``MinMaxDeque`` windows for jitter and lateness,
   and publishes derived scalars to relaxed atomics for the pull
   snapshot. The executor's ``stats`` module carries only the std-side
   push/pull value types (``CycleObservation``, ``StatsSnapshot``,
   ``TaskStatsEntry``).

   **In ``crates/taktora-executor/src/observer.rs``**

   * Extend ``Observer`` with a default-method
     ``fn on_cycle_stats(&self, _: &CycleObservation) {}`` — the
     default no-op preserves backward compatibility for existing
     ``Observer`` implementations.

   **In ``crates/taktora-executor/src/executor.rs``**

   * Add a ``Vec<ExecutorCycleStats>`` field on ``Executor``, sized at
     ``build`` time from the registered-task count. Pre-allocate per
     :need:`REQ_0060`.
   * In the ``dispatch_loop`` post-execute integration: fold
     ``took``, ``jitter``, and ``lateness`` into the task's
     ``ExecutorCycleStats`` via ``record_cycle(...)`` — windowed max
     uses ``MinMaxDeque`` (not ``fetch_max``), then call
     ``observer.on_cycle_stats(&obs)``. The pre-existing
     ``overrun_count`` counter (:need:`REQ_0102`) is read at snapshot
     time.
   * Add public ``Executor::stats_snapshot(&self) -> StatsSnapshot``
     that reads the published relaxed atomics from each
     ``ExecutorCycleStats`` and assembles the snapshot.

   **Verification**

   * Histogram accuracy — :need:`TEST_0190`.
   * Jitter readout — :need:`TEST_0191`.
   * Overrun counter — :need:`TEST_0192`.
   * Push/pull contract — :need:`TEST_0193`.
   * Allocation-free update — :need:`TEST_0194`.

.. arch-decision:: Shared no_std taktora-stats crate
   :id: ADR_0062
   :status: open
   :refines: REQ_0104

   **Context.** The allocation-free statistics primitive (fixed-bucket
   histogram per :need:`ADR_0060`, the monotonic min/max deque of
   :need:`REQ_0105`, the atomic aggregate fields) is needed in two
   places: the executor's scan-cycle stats (:need:`BB_0050`) and the
   connector's cycle telemetry (:need:`ADR_0063`). The connector seam
   ``taktora-cyclic-fieldbus`` is ``#![no_std]`` with zero dependencies,
   so any primitive it reuses must itself be ``no_std`` and
   allocation-free. The original design (``IMPL_0070``) placed the
   histogram inside ``taktora-executor`` (a ``std`` crate).

   **Decision.** Extract the primitive into a new ``#![no_std]``,
   zero-dependency, allocation-free workspace crate ``taktora-stats``,
   depended on by both ``taktora-executor`` and the connector layer.
   ``taktora-executor``'s ``stats`` module becomes a thin ``std``-side
   wrapper that adds the ``Instant`` clock reads and the ``Observer``
   wiring; the math lives once in ``taktora-stats``.

   **Alternatives considered.**

   * *Keep stats in ``taktora-executor``, duplicate for the connector.*
     Avoids a new crate, but forks the allocation-free histogram logic
     into two implementations that must be kept bit-identical and both
     pass :need:`TEST_0194`-style allocation audits. Rejected: the
     primitive is exactly the kind of subtle, invariant-heavy code that
     must not be duplicated.
   * *Put the primitive in ``taktora-cyclic-fieldbus``.* Would avoid a
     new crate name, but burdens the fieldbus seam with statistics
     concerns and inverts the dependency (the executor would depend on a
     fieldbus crate for stats). Rejected on layering grounds.

   **Consequences.**

   ✅ One allocation-free implementation, one :need:`TEST_0194` audit,
   reused at both layers.
   ✅ ``no_std`` from the start keeps the primitive usable on the
   connector seam and any future embedded target.
   ❌ One more workspace crate to version and publish. Acceptable; the
   crate is small and stable once the bucket layout is fixed.

.. building-block:: taktora-stats crate
   :id: BB_0053
   :status: open
   :implements: REQ_0104, REQ_0105
   :refines: ADR_0062

   The ``taktora-stats`` workspace crate. ``#![no_std]``, zero runtime
   dependencies. Public surface:

   * ``Histogram`` — fixed log-linear bucket table (:need:`ADR_0060`);
     ``record(value_ns)`` (``#[inline]``, allocation-free),
     ``percentile(q) -> u64``, snapshot-ring windowing.
   * ``MinMaxDeque`` — fixed-capacity monotonic deque pair giving exact
     windowed min/max (:need:`REQ_0105`); ``record(value)`` amortised
     O(1), ageing by sequence index.
   * Atomic aggregate helpers (``fetch_max`` over ``AtomicU64`` /
     ``AtomicI64``) for the max-jitter / max-lateness / overrun fields.

   Consumed by :need:`BB_0050` (executor) and the connector telemetry
   building blocks (:need:`ADR_0063`).

----

PREEMPT_RT validation harness
-----------------------------

Detailed design for the **PREEMPT_RT validation harness** sub-feature
(:need:`FEAT_0022`). The harness is packaged as an out-of-tree cargo
bin and consumes the :need:`FEAT_0021` telemetry push channel as its
sole measurement path.

.. arch-decision:: Harness as xtask, not CI gate
   :id: ADR_0061
   :status: open
   :refines: REQ_0112

   **Context.** :need:`REQ_0110` requires a documented worst-case
   jitter envelope. The natural ASPICE / industrial pattern is to wire
   a benchmark gate into CI so regressions block merge. Cloud
   GitHub-hosted runners do not run PREEMPT_RT and cannot be made to
   do so without self-hosting. A self-hosted PREEMPT_RT runner for a
   single-maintainer personal project carries ongoing infra cost
   (host availability, kernel updates, runner-agent updates).

   **Decision.** Package the harness as an out-of-tree cargo bin
   under ``xtask/preempt-rt/`` and document a manual reproduction
   procedure (per :need:`REQ_0112`). Do not gate CI on jitter
   measurements. The envelope artifact (:need:`REQ_0110`) is updated
   manually after a measurement run.

   **Alternatives considered.**

   * *Self-hosted PREEMPT_RT runner with auto-gate.* Captures
     regressions automatically but introduces a single-point-of-
     failure infra dependency. Rejected for the current
     single-maintainer setup; revisitable once the project has
     persistent infrastructure.
   * *Scheduled (nightly) run on self-hosted runner.* Same infra
     dependency as the auto-gate, with slower regression detection.
     Rejected for the same reason.
   * *Run ``cyclictest`` only, no harness.* Loses the link between
     measurements and the ``taktora-executor`` dispatch path. Rejected
     because the relevant question is "what jitter does taktora add on
     top of the kernel?", which ``cyclictest`` alone cannot answer.

   **Consequences.**

   ✅ Zero ongoing infra cost; runs are on-demand by the maintainer.
   ✅ The harness path is identical to the production telemetry path
   (per :need:`REQ_0113`), so the manual run is representative of
   production behaviour.
   ❌ Regressions can land between manual runs. Mitigated partly by
   :need:`TEST_0194` (allocation-free telemetry update) and
   :need:`TEST_0192` (overrun counter correctness) staying in regular
   CI; what the harness uniquely validates is the *absolute envelope*,
   not behavioural correctness.

.. arch-decision:: Motion-flavored adapted reference workload
   :id: ADR_0064
   :status: open
   :refines: REQ_0111

   **Context.** :need:`REQ_0111` requires a representative, repeatable
   load profile for the jitter harness. The recognised prior art is the
   ROS 2 real-time working group reference system: a fixed,
   version-controlled node graph (sensor / transform / fusion / cyclic /
   command archetypes) with a designated hot path, a per-node CPU
   calibration tool, and a defined KPI set (hot-path latency, cyclic-node
   period jitter, dropped samples). Two postures: a faithful port of
   that graph (so taktora numbers compare apples-to-apples with published
   reference-system results), or an adapted graph shaped for motion
   control.

   **Decision.** Adapt, do not faithfully port. Reuse the reference
   system's *node archetypes*, *KPI definitions*, and *per-node CPU
   calibration* methodology, but lay out a smaller topology shaped like a
   motion-control application (a cyclic NC-style node on the hot path,
   feeding setpoints; auxiliary sensor/fusion nodes off the hot path).

   **Alternatives considered.**

   * *Faithful port of the full reference-system graph.* Yields direct
     cross-framework comparability ("taktora executor vs other executors
     on the standard graph"). Rejected as the primary harness because the
     graph is autonomy-perception-shaped, not motion-shaped; the hot path
     and node mix do not resemble a taktora motion deployment, so the
     headline numbers would not characterise the load taktora actually
     runs.
   * *Bespoke topology from scratch, no reference-system lineage.*
     Maximum freedom, but discards the reference system's hard-won KPI
     definitions and calibration discipline and invites
     ad-hoc/unrepeatable load. Rejected.

   **Consequences.**

   ✅ The measured load resembles a real taktora motion deployment, so
   the envelope is meaningful for the product's actual use.
   ✅ KPI definitions and per-node calibration are inherited, keeping the
   harness rigorous and tier-portable.
   ❌ Numbers are **not** directly comparable to published
   reference-system executor results (the graph differs). Documented as a
   deliberate trade: domain relevance over cross-framework comparability.

.. building-block:: xtask-preempt-rt harness
   :id: BB_0052
   :status: open
   :implements: REQ_0111
   :refines: ADR_0061, ADR_0064

   Workspace member ``xtask-preempt-rt`` — a cargo bin that constructs
   the motion-flavored reference topology (:need:`ADR_0064`), runs it
   for a configurable number of scan cycles, and writes
   ``CycleObservation`` records to stdout as NDJSON.

   * **Workload.** The reference topology of :need:`ADR_0064` — a fixed
     graph of motion-shaped node archetypes with a designated hot path —
     not an ad-hoc executor. Per-node synthetic CPU work is tuned by a
     ``number_cruncher``-style calibration step so the absolute load is
     comparable across the dev / Pi5 / PREEMPT_RT tiers.
   * **Warm-up.** The first N scan cycles (configurable) are discarded
     before statistics are collected, so cache/page-fault warm-up does
     not contaminate the steady-state envelope.
   * **Usage.** Runs on all three tiers, but as a **local** developer
     tool only — it is never wired as a blocking cloud-CI gate, per
     :need:`ADR_0061`. Cloud runners are neither PREEMPT_RT nor quiet
     enough to measure jitter reliably; the published envelope
     (:need:`REQ_0110`) comes from a manual run on a tuned target.

   CLI shape:

   .. code-block:: text

      cargo xtask preempt-rt-bench \
          --load-profile {idle,cpu-stress,cyclictest-coexist} \
          --cycle-count <N> \
          --task-count <K> \
          --scan-period-us <P>

   The harness installs a custom ``Observer`` implementation whose
   ``on_cycle_stats`` writes one NDJSON line per call. No timing
   measurements are taken outside the ``Observer`` callback
   (per :need:`REQ_0113`).

.. impl:: xtask-preempt-rt — crate layout and procedure doc
   :id: IMPL_0071
   :status: open
   :implements: BB_0052
   :refines: REQ_0111

   **New workspace member ``xtask/preempt-rt/``**

   * ``Cargo.toml`` — depends on ``taktora-executor`` plus minimal
     transitive crates. Not a default workspace build target.
   * ``src/main.rs`` — argument parsing (``clap``), executor
     construction, ``Observer`` wiring, run loop.
   * ``src/workload.rs`` — load-profile fixtures
     (``idle``, ``cpu-stress``, ``cyclictest-coexist``).
     ``cpu-stress`` spawns ``stress-ng``; ``cyclictest-coexist`` prints
     a copy-paste ``cyclictest`` command and waits for the operator.
   * ``src/ndjson.rs`` — minimal NDJSON writer (no ``serde_json``
     dependency to keep the harness's own jitter low).

   **New document ``docs/preempt-rt-procedure.md``** (deferred to
   the implementation phase — written when the first measurement run
   is staged so the procedure can reflect the actual host).

   Sections planned:

   * Prerequisites — Debian / Ubuntu host with
     ``linux-image-rt-amd64`` or equivalent, ``stress-ng``,
     ``rt-tests``.
   * Kernel configuration — ``CONFIG_PREEMPT_RT=y`` verification,
     boot-line flags (``isolcpus=2,3``, ``nohz_full=2,3``,
     ``rcu_nocbs=2,3``).
   * Capability and pinning — ``CAP_SYS_NICE`` requirement for
     ``SCHED_FIFO`` (per :need:`REQ_0041`).
   * Reproducing the envelope — sample command line for each load
     profile.
   * Updating the envelope artifact — how to incorporate fresh
     measurements into :need:`REQ_0110`'s versioned document.

   **Verification**

   * Build + smoke run — :need:`TEST_0240`.
   * NDJSON schema — :need:`TEST_0241`.
   * Push/pull agreement — :need:`TEST_0242`.

Cycle-overrun fault primitive (FEAT_0018)
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

.. building-block:: Cycle-overrun fault primitive surface
   :id: BB_0093
   :status: implemented
   :implements: FEAT_0018

   New module :code:`crates/taktora-executor/src/fault.rs` owning
   :code:`FaultState`, :code:`ExecutorFaultState`, packed
   :code:`AtomicU64` storage, and the post-execute detection hook
   consumed by the executor.

.. impl:: Per-task fault state machine
   :id: IMPL_0081
   :status: implemented
   :implements: REQ_0070, REQ_0102

   Implementation in :code:`crates/taktora-executor/src/fault.rs`
   (FaultAtomic, FaultState, FaultReason) plus the post-execute hook
   in :code:`crates/taktora-executor/src/executor.rs::post_execute_detect_fault`.

.. impl:: Executor-wide fault state machine
   :id: IMPL_0082
   :status: implemented
   :implements: REQ_0071

   Implementation in :code:`crates/taktora-executor/src/fault.rs`
   (ExecutorFaultAtomic, ExecutorFaultState, ExecutorFaultReason)
   plus the executor-wide breach detection in
   :code:`post_execute_detect_fault` and lazy cascade in
   :code:`dispatch_loop`.

.. impl:: Fault state Observer callbacks
   :id: IMPL_0083
   :status: implemented
   :implements: REQ_0073

   Four new :code:`Observer` methods in
   :code:`crates/taktora-executor/src/observer.rs` plus their
   forwards in :code:`crates/taktora-executor-tracing/src/lib.rs`.

.. impl:: Fault handler dispatch path
   :id: IMPL_0084
   :status: implemented
   :implements: REQ_0072

   New :code:`Executor::add_with_fault_handler` registration path and
   :code:`build_handler_job` closure builder in
   :code:`crates/taktora-executor/src/executor.rs`, plus the
   pre-dispatch routing decision in :code:`dispatch_loop`.

----

Framework internal-fault model (FEAT_0024)
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

.. arch-decision:: Abort on framework-invariant violation; watchdog drives outputs safe
   :id: ADR_0065
   :status: open
   :refines: REQ_0123

   **Context.** The cyclic dispatch path has two nested
   :code:`catch_unwind` layers. The **inner** layer
   (:code:`run_item_catch_unwind`, ``executor.rs``) wraps each user
   item and converts a task panic into a ``PanickedTask`` error that
   drives the :need:`FEAT_0018` fault machine — the task-isolation
   guarantee of :need:`AFSR_0004`. The **outer** layer (the pool worker
   loop and inline-submit path, ``pool.rs``) wraps every job and today
   **swallows** whatever it catches (:code:`let _ = catch_unwind(...)`).

   Because user-item panics are already neutralised by the inner layer,
   the *only* panics that can reach the outer layer are framework-
   internal: a poisoned dispatch ``Mutex`` (``first_err``, ``done_cv``,
   ``iter_err``), a ``ready_ring`` overflow, broken in-degree
   accounting. Swallowing these is actively dangerous: e.g. a panic in
   the ``ready_ring.push().expect()`` path leaves ``pending``
   decremented but successors un-enqueued, so :code:`run_once_borrowed`
   spins on its 5 ms ``wait_timeout`` **forever** — a silent cyclic-task
   hang with outputs frozen at their last value and **no fault
   surfaced**, violating :need:`AFSR_0004`. In a control loop a frozen
   actuator is an undefined-state event.

   The runtime stays on ``panic = "unwind"`` globally — the inner
   catch-and-fault mechanism depends on unwinding, so a global
   ``panic = "abort"`` is not an option.

   **Decision.** Treat any panic reaching the outer (framework) boundary
   as a non-recoverable internal-invariant violation and **fail fast**:
   invoke a best-effort, time-bounded user fatal handler
   (:need:`REQ_0125`), then :code:`std::process::abort`. The boundary is
   installed at every runtime-thread top — pool worker loop, inline
   submit, and the executor dispatch thread's run loop. User-item panics
   continue to be caught and faulted at the inner layer (:need:`REQ_0124`),
   never reaching the abort path.

   The **documented output failure model** on abort is: ``abort`` runs
   no destructors (so :code:`EthercatGateway::Drop`'s graceful tokio
   shutdown does *not* run) → the master thread stops emitting
   process-data frames → each output slave's sync-manager watchdog
   expires → the slave drops OP → SAFE-OP and applies its configured
   safe-state values. Outputs hold their last commanded value for up to
   the watchdog timeout, then go safe — with **zero dependency on
   taktora code running after the violation**. This robustness is the
   point: the safe-state path cannot be defeated by the corrupt state
   that triggered the abort. Its load-bearing precondition is
   :need:`AOU_0016` (watchdog enabled, timeout ≤ FTTI/2).

   **Alternatives considered.**

   * *Controlled stop / run the fault handler (REQ_0072) over the broken
     state.* Rejected: once a dispatch invariant is violated the locks,
     ring, and in-degree counters are untrustworthy; executing more
     framework logic over them — including a fault handler — is less
     safe than aborting, and the watchdog already provides the
     output-safe guarantee without it.
   * *Global* ``panic = "abort"``. Rejected: deletes the inner
     catch-and-fault path, collapsing per-task isolation so one task's
     panic kills the whole control process.
   * *Best-effort "drive outputs safe" frame before abort.* Rejected:
     runs master code over state just declared untrustworthy, for a
     guarantee the slave watchdog already provides. (A *narrow*
     last-gasp that does **not** touch executor internals — GPIO pin,
     black-box flush — is permitted via the :need:`REQ_0125` handler.)
   * *Static enforcement of the watchdog bound now.* Deferred, not
     rejected: the SM watchdog is not modelled in
     ``taktora-ethercat-esi`` / ``taktora-ethercat-netcfg`` today, so
     the ≤ FTTI/2 bound cannot be validated at config time. The bound
     is recorded as :need:`AOU_0016`; modelling + validation is a
     separate dependent slice.

   **Consequences.**

   ✅ Infrastructure panics can no longer silently hang the executor;
   they become an immediate, observable process abort.
   ✅ The fail-fast path is exercisable in CI via an injected fatal
   handler (:need:`REQ_0125`), so it does not rot.
   ✅ The output-safe guarantee depends on no post-panic taktora code.
   ❌ The output-safe timing is bounded by the slave watchdog, not by
   taktora; correctness rests on :need:`AOU_0016` holding. Enforcement
   of the ≤ FTTI/2 bound is deferred until the SM watchdog is modelled.
   ❌ ``abort`` skips all destructors process-wide; any non-watchdog
   cleanup (e.g. log flush) must be done in the :need:`REQ_0125`
   handler.

.. building-block:: Framework fail-fast boundary
   :id: BB_0094
   :status: open
   :implements: FEAT_0024

   The outer (framework) panic boundary, realised at every runtime
   thread top: the pool worker loop and inline-submit path in
   :code:`crates/taktora-executor/src/pool.rs`, and the executor
   dispatch thread's run loop in
   :code:`crates/taktora-executor/src/executor.rs`. Each converts a
   caught panic into a call through the registered fatal handler
   followed by :code:`std::process::abort`, replacing today's
   :code:`let _ = catch_unwind(...)` swallow. Carries the
   ``on_fatal`` registration on :code:`ExecutorBuilder` and the
   ``FatalContext`` cause type.

.. impl:: Fail-fast boundary and fatal handler
   :id: IMPL_0085
   :status: open
   :implements: REQ_0123, REQ_0125

   Replace the swallowing :code:`let _ = catch_unwind(...)` in
   ``pool.rs`` (worker loop and inline submit) and wrap the executor
   dispatch thread's run loop, routing a caught payload through
   :code:`Executor`'s registered ``on_fatal`` handler (default no-op,
   itself catch-guarded) then :code:`std::process::abort`. Add the
   ``on_fatal`` builder setter and ``FatalContext`` (captured payload
   message + thread/site label). Function-scoped
   :code:`#[deny(clippy::unwrap_used, clippy::expect_used,
   clippy::panic)]` on the cyclic-path fns, with each intentional
   fail-fast site annotated :code:`#[allow(...)] // fail-fast: <invariant>`.

.. impl:: User-item panic containment
   :id: IMPL_0086
   :status: implemented
   :implements: REQ_0124

   Existing :code:`run_item_catch_unwind` in
   :code:`crates/taktora-executor/src/executor.rs` — retro-documented:
   catches the item panic and builds a ``PanickedTask`` ``ItemError``,
   which the dispatch paths surface via ``Observer::on_app_error`` and
   propagate as the item's error result (stopping downstream items per
   :need:`REQ_0022`). It does **not** drive the :need:`FEAT_0018`
   ``Faulted`` state — that is reserved for deadline breaches
   (:need:`REQ_0070`) — and it never reaches the fail-fast boundary of
   :need:`REQ_0123`.
