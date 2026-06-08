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

   **Fixture.** Three executor configurations covering the three
   dispatch paths:

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

   4. Repeat for each of the four fixture configurations
      above.

   **Expected outcome.** All four assertions hold:
   ``per_iter == 0``. Test passes under ``cargo test
   -p taktora-executor --test no_alloc_dispatch --release``.

   **Negative case.** ``harness_catches_deliberate_allocation``
   registers a task whose ``execute`` body does
   ``vec![1, 2, 3]`` per iteration and asserts that the
   counting allocator records ``≥ 10`` allocations across 10
   iterations — guards against silent harness regressions
   where the ``#[global_allocator]`` is not actually wired up.

   Lives under
   ``crates/taktora-executor/tests/no_alloc_dispatch.rs``.

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

   **Fixture.** ``crates/taktora-executor/tests/no_alloc_dispatch.rs``
   — ``dispatch_is_zero_allocation`` (lines 76-166). Uses the
   process-wide ``CountingAllocator`` as the ``#[global_allocator]``
   to count allocations on every thread (WaitSet plus pool workers)
   inside a bracketed measurement window. The differential
   ``per_iter = ceil((alloc(run_n(100)) - alloc(run_n(10))) / 90)``
   isolates per-iteration allocations from one-shot setup.

   **Steps.**

   1. Build any of the four fixture executors
      (single-threaded chain, two-worker chain, two-worker
      diamond graph, single-threaded single item).
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

   **Expected outcome.** ``per_iter == 0`` across all four
   fixture configurations — the executor's pre-allocated
   ``iter_err: Arc<Mutex<Option<ExecutorError>>>`` (built once
   in ``Executor::build``) is reused, not re-allocated, on
   every dispatch iteration.
