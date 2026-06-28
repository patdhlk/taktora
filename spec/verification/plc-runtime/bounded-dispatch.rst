Bounded-time dispatch
=====================

Test cases verifying the bounded-time dispatch sub-feature
(:need:`FEAT_0017`): the zero-allocation steady-state dispatch guarantee
(:need:`REQ_0060`) and the pre-allocated per-iteration error slot
(:need:`REQ_0062`).

Zero-allocation dispatch
------------------------

.. test:: Zero allocations in steady-state dispatch
   :id: TEST_0170
   :status: open
   :verifies: REQ_0060

   **Goal.** Confirm that **steady-state** iterations of
   ``Executor::run_n`` perform **zero** heap allocations on
   any thread (WaitSet thread + pool worker threads).
   "Steady-state" excludes the one-time setup that
   ``dispatch_loop`` performs each ``run_n`` entry (WaitSet
   construction, trigger attachment, iceoryx2 lazy init); the
   harness isolates per-iteration allocations from setup
   allocations via a differential measurement.

   **Fixture.** Five executor configurations covering the
   dispatch paths (chain, single, graph) plus the event/fd
   resolve path through ``AttachmentMap``:

   * ``Executor::builder().worker_threads(0).build()`` + ``add_chain([h, m, t])`` —
     ``TaskKind::Chain`` on the inline pool.
   * ``Executor::builder().worker_threads(2).build()`` + ``add_chain([h, m, t])`` —
     ``TaskKind::Chain`` on the threaded pool.
   * ``Executor::builder().worker_threads(0).build()`` + ``add(single_item)`` —
     ``TaskKind::Single`` on the inline pool.
   * ``Executor::builder().worker_threads(2).build()`` + diamond ``add_graph`` —
     ``TaskKind::Graph`` on the threaded pool (vertex
     dispatch via per-vertex pre-built closures + SPSC
     ring).
   * ``Executor::builder().worker_threads(0).dispatch_mode(Legacy).build()`` +
     ``add(interval_item)`` — forces the interval to attach as a WaitSet Tick
     guard so dispatch routes through ``AttachmentMap::resolve`` on the
     positive branch (precomputed Tick id -> real task index, a binary-search
     hit), exercising the ``O(log n)`` resolver's hot path allocation-free on
     every platform. Unlike the Grid default (Linux), where intervals divert
     to the master-timer ``run_grid_cyclic_pass`` path and never touch the
     map, this case pins ``Legacy`` so the resolve branch runs everywhere.
     See :need:`REQ_0060` / ADR_0106.

   Each item / vertex returns ``Ok(Continue)`` without
   allocating.

   **Allocator instrumentation.** A hand-rolled counting
   ``#[global_allocator]`` (``CountingAllocator``) wraps
   ``std::alloc::System``. Two atomics — ``ALLOC_COUNT`` and
   ``TRACKING`` — are flipped on / off around the measurement
   window. Every thread (including pool workers) increments
   ``ALLOC_COUNT`` on alloc / realloc / alloc_zeroed when
   ``TRACKING`` is set. This covers paths that
   thread-local-flag schemes (``assert_no_alloc``) cannot
   reach.

   **Steps.**

   1. Build the executor; register the task / chain / graph.
   2. ``per_iter_allocs(&mut exec)``:

      a. Warm up with ``run_n(10)`` (untracked) to absorb any
         one-shot lazy init (iceoryx2 service handles
         first-touched on the WaitSet thread, etc.).
      b. Bracket ``run_n(10)`` with the counting allocator
         and record ``a_small``.
      c. Bracket ``run_n(100)`` with the counting allocator
         and record ``a_big``.
      d. Return ``ceil((a_big - a_small) / (100 - 10))`` —
         the average steady-state allocations per dispatch
         iteration, with setup-phase allocations subtracted
         out via the differential.

   3. Assert ``per_iter == 0``.

   4. Repeat for each of the five fixture configurations
      above.

   **Expected outcome.** All five assertions hold:
   ``per_iter == 0``. Test passes under ``cargo test
   -p taktora-executor-tests --test no_alloc_dispatch --release``.

   .. note::

      **Platform scope of the worker-thread fixtures (issue #132).** The
      two ``worker_threads(2)`` fixtures (threaded-pool chain and diamond
      graph) assert ``per_iter == 0`` strictly on **Linux only**. The
      process-wide ``CountingAllocator`` counts allocations on the pool
      worker threads too, and on macOS the workers' ``crossbeam_channel`` /
      ``Condvar`` park-notify path occasionally allocates inside
      ``libsystem_malloc``; that single allocation is charged
      non-deterministically to one of the two differential windows but not
      the other, producing a spurious ``diff == 1`` off-by-one. This is
      allocator/scheduler noise, not a product regression — the dispatch
      hot path is provably zero-alloc (a 10-iter and a 100-iter window
      record identical counts). The three single-threaded
      (``worker_threads(0)``) fixtures have no pool threads, are
      deterministic on every platform, and keep the strict assertion
      everywhere — so :need:`REQ_0060` remains enforced on macOS for the
      single-threaded dispatch path and on Linux for **all** paths.

   **Negative case.** ``harness_catches_deliberate_allocation``
   registers a task whose ``execute`` body does
   ``vec![1, 2, 3]`` per iteration and asserts that the
   counting allocator records ``≥ 10`` allocations across 10
   iterations — guards against silent harness regressions
   where the ``#[global_allocator]`` is not actually wired up.

   Lives under
   ``crates/taktora-executor-tests/tests/no_alloc_dispatch.rs``.

Bounded-time dispatch (pre-allocated error slot)
------------------------------------------------

Additional verification under the bounded-time dispatch sub-feature
(:need:`FEAT_0017`) — the pre-allocated per-iteration error slot
that :need:`REQ_0062` mandates.

.. test:: Per-iteration error slot is pre-allocated, not Arc-Mutex-allocated per cycle
   :id: TEST_0141
   :status: implemented
   :verifies: REQ_0062

   **Goal.** Confirm the dispatch loop does not construct a fresh
   ``Arc<Mutex<Option<ExecutorError>>>`` per iteration — the
   anti-pattern :need:`REQ_0062` forbids. Verified indirectly: any
   per-iteration ``Arc`` construction would surface as a non-zero
   ``per_iter`` allocation count under the differential measurement
   in :need:`TEST_0170`.

   **Fixture.** ``crates/taktora-executor-tests/tests/no_alloc_dispatch.rs``
   — ``dispatch_is_zero_allocation`` (lines 76-194). Uses the
   process-wide ``CountingAllocator`` as the ``#[global_allocator]``
   to count allocations on every thread (WaitSet plus pool workers)
   inside a bracketed measurement window. The differential
   ``per_iter = ceil((alloc(run_n(100)) - alloc(run_n(10))) / 90)``
   isolates per-iteration allocations from one-shot setup.

   **Steps.**

   1. Build any of the five fixture executors
      (single-threaded chain, two-worker chain, two-worker
      diamond graph, single-threaded single item,
      single-threaded Legacy interval).
   2. Warm up with an untracked ``run_n(10)``.
   3. Bracket ``run_n(10)`` and ``run_n(100)`` with the counting
      allocator.
   4. Compute the differential ``per_iter`` and assert it equals
      ``0``.

   **Negative case.** A deliberate ``vec![1, 2, 3]`` allocation
   inside an item body surfaces as ``allocs >= 10`` over 10
   iterations, proving the harness catches per-iteration
   allocations (which is what would happen if the error slot
   were Arc-Mutex-allocated per cycle).

   **Expected outcome.** ``per_iter == 0`` across all five
   fixture configurations — the executor's pre-allocated
   ``iter_err: Arc<Mutex<Option<ExecutorError>>>`` (built once
   in ``Executor::build``) is reused, not re-allocated, on
   every dispatch iteration.

Per-phase dispatch dedup (at-most-one borrowed-job submit)
----------------------------------------------------------

Verification of the at-most-one-submit-per-barrier-phase contract
(:need:`REQ_0854`) and the exactly-one-scan-period rejection that
:need:`REQ_0002` mandates — the guard recorded in :need:`ADR_0105`.

.. test:: At-most-one borrowed-job submit per barrier phase; multiple intervals rejected
   :id: TEST_0872
   :status: implemented
   :verifies: REQ_0854

   **Goal.** Pin two facets of the per-phase dedup contract: (1) a task
   with two attachments fired in one wake-phase is **submitted once** — its
   single run drains all pending input — so two pool workers never alias one
   ``*mut dyn FnMut``, covering both the main item and the borrowed fault
   handler; and (2) declaring more than one ``interval()`` on a single task
   is **rejected at ``add`` / ``Executor::build`` time**, the behavioral
   break of :need:`REQ_0002`.

   **Fixture.** In-crate unit tests in
   ``crates/taktora-executor/src/executor.rs``:

   * ``add_rejects_multiple_intervals`` — registering a single item that
     declares two ``interval()`` triggers returns a build/add error.
   * ``add_chain_rejects_multiple_intervals`` — the same rejection on the
     ``add_chain`` path, so a chain cannot smuggle in a second scan period.
   * ``dispatch_guard_runs_event_task_once_per_phase`` — an event task with
     two listeners fired in one wake-phase runs **once** (its ``take()``
     loop drains both notifications) rather than being submitted twice; the
     guard short-circuits on ``pending_cycle.is_some()`` before fault
     routing.
   * ``dispatch_guard_runs_fault_handler_once_per_phase`` — the borrowed
     **fault-handler** submit is likewise guarded: a task routed to its
     fault handler with the token already set this phase is skipped, so the
     fault handler is submitted at most once per phase.

   **Steps.**

   1. *Rejection.* Build an executor and register a task declaring two
      ``interval()`` triggers via ``add`` (then ``add_chain``); assert the
      call returns the multiple-interval rejection error and the executor is
      not built with the illegal task.
   2. *Event dedup.* Build a ``worker_threads(2)`` executor with one event
      task carrying two listeners; arrange both listeners to be notified in
      a single wake-phase; run one phase and read a run counter.
   3. *Fault dedup.* Drive a task into its fault-routed branch within a
      phase where ``pending_cycle`` is already set; read the fault-handler
      run counter.

   **Expected outcome.**

   * Both ``*_rejects_multiple_intervals`` tests observe the rejection error
     at add/build time.
   * The event task's run counter advances by **exactly one** for the
     two-listener wake-phase (one submit, ``take()`` drains both), and
     ``cycle_index`` / recorded cycles advance at most once for the phase.
   * The fault handler is submitted **at most once** per phase — the second,
     would-be-aliasing submit is skipped by the ``pending_cycle`` guard.

   Existing ``MockClock`` telemetry tests pass unchanged: ``record_cycle_for``
   early-returns for event tasks, so the single recorded cycle is the cyclic
   one and telemetry is unaffected.
