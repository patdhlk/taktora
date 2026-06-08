Cyclic scan execution
=====================

Test cases verifying the cyclic scan execution sub-feature
(:need:`FEAT_0011`).

.. test:: Interval trigger fires the configured number of times
   :id: TEST_0104
   :status: implemented
   :verifies: REQ_0001

   **Goal.** Confirm a cyclic item registered with
   ``TriggerDeclarer::interval(Duration::from_millis(20))`` is
   dispatched exactly ``n`` times by ``Executor::run_n(n)``.

   **Fixture.** ``crates/taktora-executor/tests/run_loop.rs`` —
   ``interval_trigger_fires_run_n_times`` (lines 15-36). An
   inline-pool ``Executor`` (``worker_threads(0)``) with one
   item declaring a 20 ms interval and an ``AtomicU32`` counter.

   **Steps.**

   1. Build the executor; register the item.
   2. Call ``exec.run_n(3)``.
   3. Read the counter.

   **Expected outcome.** ``counter == 3``. The interval declaration
   is honoured by the dispatch loop.

.. test:: Interval cardinality matches run_n on the threaded pool
   :id: TEST_0105
   :status: implemented
   :verifies: REQ_0002

   **Goal.** Under nominal load the interval-driven dispatch fires
   exactly once per declared period — verified by asserting the
   per-cycle count after ``run_n(N)`` equals ``N`` on the threaded
   pool, where contention could otherwise cause coalesced or
   skipped fires.

   **Fixture.** ``crates/taktora-executor/tests/run_loop.rs`` —
   ``threaded_pool_executes_items_correctly`` (lines 122-149). A
   two-worker ``Executor`` with one cyclic item at 20 ms period
   and an ``AtomicU32`` counter.

   **Steps.**

   1. Build with ``worker_threads(2)``; register the item.
   2. Call ``exec.run_n(5)``.
   3. Read the counter.

   **Expected outcome.** ``counter == 5`` — exactly one execute
   per fired interval, no duplicates and no drops.

.. test:: ExecutionMonitor brackets every dispatch
   :id: TEST_0106
   :status: implemented
   :verifies: REQ_0003

   **Goal.** ``ExecutionMonitor::pre_execute`` is invoked once and
   ``post_execute`` is invoked once per scan-cycle dispatch, in
   matched pairs.

   **Fixture.** ``crates/taktora-executor/tests/monitor.rs`` —
   ``monitor_brackets_each_execute`` (lines 26-48). A
   ``RecordingMonitor`` (``AtomicU32`` for ``pre`` and ``post``,
   plus a ``Mutex<Vec<(TaskId, Duration, bool)>>`` of recorded
   timings) attached to an inline-pool executor with a single
   cyclic item at 10 ms period.

   **Steps.**

   1. Build the executor with the monitor; register the cyclic
      item returning ``Ok(Continue)``.
   2. Call ``exec.run_n(3)``.
   3. Read ``mon.pre``, ``mon.post``, and ``mon.times``.

   **Expected outcome.** ``pre == 3``, ``post == 3``, and every
   recorded triple has ``ok == true``.
