PLC runtime — verification
==========================

Test cases verifying the PLC runtime heart family (:need:`FEAT_0010`).
Coverage today: the bounded-time dispatch sub-feature
(:need:`FEAT_0017`) and its zero-allocation requirement
(:need:`REQ_0060`); the scan-cycle observability sub-feature
(:need:`FEAT_0021`); and the PREEMPT_RT validation harness sub-feature
(:need:`FEAT_0022`).

----

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

----

Scan-cycle observability
------------------------

Test cases verifying the scan-cycle observability sub-feature
(:need:`FEAT_0021`).

.. test:: Histogram percentile accuracy
   :id: TEST_0190
   :status: open
   :verifies: REQ_0100

   **Goal.** Confirm the :need:`ADR_0060` histogram returns p50, p95,
   p99 values within the documented relative-error bound when fed a
   known reference distribution.

   **Fixture.** A standalone unit test in
   ``crates/taktora-executor/src/stats/histogram.rs`` that drives the
   ``Histogram`` directly (no full executor).

   **Steps.**

   1. Build a ``Histogram`` with the production bucket table.
   2. Feed it 10 000 samples drawn from a known distribution
      (uniform on ``[100 ns, 100 ms]`` and exponential with mean
      ``1 ms``).
   3. Compute exact percentile values from the input samples and
      compare to ``Histogram::percentile(q)`` for q ∈ {0.5, 0.95,
      0.99}.
   4. Assert relative error ≤ 1% (bucket centroid bound) for each
      percentile in each distribution.

   **Expected outcome.** All twelve assertions hold (3 quantiles × 2
   distributions × 2 runs for stability).

   Lives under
   ``crates/taktora-executor/src/stats/histogram.rs`` ``#[cfg(test)]``.

.. test:: Per-task max jitter under synthetic period violation
   :id: TEST_0191
   :status: open
   :verifies: REQ_0101

   **Goal.** A synthetic period violation produces the correct
   max-jitter readout.

   **Fixture.** Executor with one cyclic task at 10 ms scan period.
   The task body sleeps for a configurable extra delay on selected
   cycles to induce period jitter.

   **Steps.**

   1. Build executor, register cyclic task with 10 ms period.
   2. Run 100 cycles where the task adds a 3 ms delay on every
      10th cycle.
   3. Query ``Executor::stats_snapshot``; read
      ``per_task[0].max_jitter_ns``.
   4. Assert ``max_jitter_ns ≥ 3 ms - timer-resolution-margin`` and
      ``max_jitter_ns ≤ 3 ms + timer-resolution-margin``.

   **Expected outcome.** Max jitter falls within the expected band.

   Lives under
   ``crates/taktora-executor/tests/cycle_stats_max_jitter.rs``.

.. test:: Overrun counter increments exactly per overrun cycle
   :id: TEST_0192
   :status: open
   :verifies: REQ_0102

   **Goal.** ``overrun_count`` increments exactly once per cycle that
   exceeds the declared scan period, and not at all on cycles within
   the period.

   **Fixture.** Executor with one cyclic task at 10 ms period.

   **Steps.**

   1. Run 50 cycles where the task body completes in 1 ms.
      Assert ``overrun_count == 0``.
   2. Run 30 cycles where the task body deliberately takes 15 ms
      (overrun by 5 ms). Assert ``overrun_count == 30``.
   3. Run 20 more cycles at 1 ms each. Assert
      ``overrun_count == 30`` (no further increments).

   **Expected outcome.** All three assertions hold.

   Lives under
   ``crates/taktora-executor/tests/cycle_stats_overruns.rs``.

.. test:: Push and pull stat paths agree
   :id: TEST_0193
   :status: open
   :verifies: REQ_0103

   **Goal.** Each completed scan cycle delivers exactly one
   ``Observer::on_cycle_stats`` callback, and the aggregate visible
   to ``stats_snapshot`` reflects every observation pushed.

   **Fixture.** Executor with two cyclic tasks (5 ms and 7 ms scan
   periods) and a custom ``Observer`` that records every
   ``on_cycle_stats`` invocation into a thread-safe ring.

   **Steps.**

   1. Run for 200 cycles total.
   2. Assert the recorded callback count matches the number of
      completed scan cycles per task.
   3. Compute the percentile from the recorded raw samples directly;
      compare against
      ``Executor::stats_snapshot().per_task[i].p95_ns`` to within
      the histogram-bucket bound.

   **Expected outcome.** Push and pull paths report consistent
   aggregates.

   Lives under
   ``crates/taktora-executor/tests/cycle_stats_push_pull.rs``.

.. test:: Allocation-free telemetry update
   :id: TEST_0194
   :status: open
   :verifies: REQ_0104

   **Goal.** The per-sample telemetry update path performs zero heap
   allocations under steady state.

   **Fixture.** Reuses the ``CountingAllocator`` from
   :need:`TEST_0170`. Executor with one cyclic task whose body is a
   no-op; the only per-cycle work on the runtime side is the
   telemetry update.

   **Steps.**

   1. Build executor; warm up with ``run_n(10)`` untracked.
   2. ``per_iter_allocs`` differential measurement over ``run_n(10)``
      vs ``run_n(100)``.
   3. Assert ``per_iter == 0``.

   **Negative case.** Replace the no-op task body with a
   ``vec![1, 2, 3]`` allocator-poisoning task; assert
   ``per_iter ≥ 1`` so the harness is verified to actually catch
   allocations.

   **Expected outcome.** Steady-state telemetry update performs zero
   heap allocations.

   Lives under
   ``crates/taktora-executor/tests/no_alloc_cycle_stats.rs``.

----

PREEMPT_RT validation harness
-----------------------------

Test cases verifying the PREEMPT_RT validation harness sub-feature
(:need:`FEAT_0022`). These tests do **not** validate the absolute
jitter envelope — that is a manual procedure per :need:`REQ_0112` and
:need:`ADR_0061`. The tests below verify that the harness itself is
well-formed (it builds, emits valid output, and agrees with the
runtime's own telemetry).

.. test:: Harness builds and runs on Linux non-RT
   :id: TEST_0240
   :status: open
   :verifies: REQ_0111

   **Goal.** The harness binary builds and runs to completion on a
   stock (non-PREEMPT_RT) Linux host without requiring elevated
   capabilities, and produces well-formed NDJSON on stdout.

   **Fixture.** GitHub Actions Linux x86_64 runner; the harness is
   built with ``cargo build --release -p xtask-preempt-rt``.

   **Steps.**

   1. Build the harness in release mode.
   2. Run ``cargo run --release -p xtask-preempt-rt --
      --load-profile idle --cycle-count 1000 --task-count 1
      --scan-period-us 1000``.
   3. Capture stdout; assert each line parses as JSON and contains
      the expected keys (``ts_ns``, ``task_id``, ``period_ns``,
      ``actual_period_ns``, ``jitter_ns``, ``took_ns``).
   4. Assert the captured line count equals ``cycle-count``.

   **Expected outcome.** Smoke run succeeds; output is well-formed.

   Lives under ``xtask/preempt-rt/tests/smoke.rs``.

.. test:: NDJSON schema validation
   :id: TEST_0241
   :status: open
   :verifies: REQ_0111

   **Goal.** The harness output conforms exactly to the documented
   NDJSON schema; no extra keys, no missing keys, correct value
   types.

   **Fixture.** An in-tree JSON Schema file
   (``xtask/preempt-rt/schema/cycle-observation.schema.json``)
   describes the record shape from :need:`REQ_0111`.

   **Steps.**

   1. Run a short harness invocation (100 cycles).
   2. Validate every output line against the schema using a
      lightweight in-tree validator (no new workspace dep — match
      keys + value-type assertions manually).
   3. Assert all 100 lines validate.

   **Expected outcome.** Output is schema-conformant.

   Lives under ``xtask/preempt-rt/tests/schema.rs``.

.. test:: Harness telemetry agrees with stats_snapshot
   :id: TEST_0242
   :status: open
   :verifies: REQ_0113

   **Goal.** The NDJSON cycle observations produced by the harness
   agree with ``Executor::stats_snapshot`` aggregates taken at the
   end of the run — i.e. the harness and the pull API see the same
   underlying data.

   **Fixture.** A test variant of the harness that, after writing
   its last NDJSON line, also writes a single ``StatsSnapshot`` JSON
   record to stderr.

   **Steps.**

   1. Run 1000 cycles with one cyclic task.
   2. Compute the percentile from the NDJSON ``took_ns`` column
      directly.
   3. Compare against the matching field in the stderr
      ``StatsSnapshot`` record.
   4. Assert agreement within the histogram-bucket bound (~1%).

   **Expected outcome.** Push and pull paths agree on the same data.

   Lives under ``xtask/preempt-rt/tests/push_pull_agreement.rs``.

----

Cyclic scan execution
---------------------

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

----

Event-driven I/O dispatch
-------------------------

Test cases verifying the event-driven I/O dispatch sub-feature
(:need:`FEAT_0012`).

.. test:: Subscriber-triggered ingestion wakes the item
   :id: TEST_0107
   :status: implemented
   :verifies: REQ_0010

   **Goal.** A ``Subscriber<T>`` declared as a trigger via
   ``TriggerDeclarer::subscriber`` causes the executor to dispatch
   the item whenever the matching ``Publisher<T>`` sends a sample.

   **Fixture.** ``crates/taktora-executor/tests/run_loop.rs`` —
   ``subscriber_trigger_dispatches_task`` (lines 83-120). An
   inline-pool executor; a unique-named iceoryx2 channel; one
   item that declares the subscriber and increments a counter
   per dispatch. A background thread publishes five
   ``Tick(u64)`` samples 20 ms apart.

   **Steps.**

   1. Open the channel; build publisher and subscriber handles.
   2. Register the item; spawn the publisher thread.
   3. Call ``exec.run()``; the item calls ``stop_executor`` after
      three fires.

   **Expected outcome.** ``counter >= 3`` — subscriber-driven
   dispatch fires at least once per delivered sample (modulo the
   stop-after-three early exit).

.. test:: Publisher API send paths deliver to attached subscribers
   :id: TEST_0108
   :status: implemented
   :verifies: REQ_0011

   **Goal.** All three ``Publisher`` send paths (``send_copy``,
   ``loan_send``, ``loan``) are present, callable, and deliver the
   payload to an attached ``Subscriber``.

   **Fixture.** ``crates/taktora-executor/tests/channel.rs`` —
   three test functions covering the three send paths:
   ``publisher_send_notifies_subscriber_listener`` (lines 17-41,
   ``send_copy``), ``publisher_loan_zero_copy_round_trip``
   (lines 52-76, ``loan``), and ``publisher_loan_skip_returns_false``
   (lines 78-101, ``loan`` skip-publish path). Each opens a
   unique-named iceoryx2 channel, constructs a ``Publisher`` and
   ``Subscriber``, and round-trips a ``Msg(u64)`` payload.

   **Steps.**

   1. ``send_copy(Msg(42))`` — read back via
      ``Subscriber::take``; assert payload value.
   2. ``loan(|slot| { slot.write(Msg(99)); true })`` — read back
      via ``Subscriber::take``; assert payload value.
   3. ``loan(|_| false)`` — assert ``outcome.sent == false`` and
      ``Subscriber::take`` returns ``None``.

   **Expected outcome.** All three send paths compile, run, and
   deliver (or correctly skip) the configured payload.

.. test:: Publisher::loan round-trips without serialisation
   :id: TEST_0109
   :status: implemented
   :verifies: REQ_0012

   **Goal.** ``Publisher::loan(|slot: &mut MaybeUninit<T>| ...)``
   writes the payload directly into the iceoryx2 shared-memory
   slot, and ``Subscriber::take`` returns an
   ``iceoryx2::sample::Sample`` whose ``payload()`` is a borrowed
   view of the same bytes — no copy, no deserialisation.

   **Fixture.** ``crates/taktora-executor/tests/channel.rs`` —
   ``publisher_loan_zero_copy_round_trip`` (lines 52-76). Single
   iceoryx2 channel; one publisher, one subscriber.

   **Steps.**

   1. ``publisher.loan(|slot| { slot.write(Msg(99)); true })``.
   2. ``subscriber.take().unwrap().expect("payload")`` returns a
      ``Sample``.
   3. Assert ``sample.payload().0 == 99``.

   **Expected outcome.** The loan path delivers the producer's
   in-place write to the consumer as a borrowed view.

.. test:: NotifyOutcome surfaces listeners-notified count
   :id: TEST_0113
   :status: implemented
   :verifies: REQ_0013

   **Goal.** Every send path on ``Publisher`` returns a
   ``NotifyOutcome { sent, listeners_notified }`` whose
   ``listeners_notified`` field reports the number of attached
   subscribers actually notified — distinguishing back-pressure
   ("no listener attached") from delivery error.

   **Fixture.** ``crates/taktora-executor/tests/channel.rs`` —
   ``publisher_send_notifies_subscriber_listener`` (lines 17-41).
   One publisher, one subscriber; a single ``send_copy``.

   **Steps.**

   1. Build channel, publisher, and subscriber.
   2. Call ``publisher.send_copy(Msg(42))``.
   3. Read ``outcome.sent`` and ``outcome.listeners_notified``.

   **Expected outcome.** ``outcome.sent == true`` and
   ``outcome.listeners_notified == 1`` — the field surfaces
   delivery accounting as a non-error counter.

----

Deterministic logic sequencing
------------------------------

Test cases verifying the deterministic logic sequencing sub-feature
(:need:`FEAT_0013`).

.. test:: Chain runs its items in declared order
   :id: TEST_0114
   :status: implemented
   :verifies: REQ_0020

   **Goal.** ``Executor::add_chain([head, mid, tail])`` invokes the
   three items strictly in declared order on a single dispatch
   slot per chain invocation.

   **Fixture.** ``crates/taktora-executor/tests/chain.rs`` —
   ``chain_runs_items_in_order`` (lines 8-43). A two-worker
   executor; three items that each push their position number
   (1, 2, 3) into a shared ``Mutex<Vec<u32>>``; the head item
   carries a 10 ms interval trigger so the chain fires once.

   **Steps.**

   1. Register the three items as one chain.
   2. Call ``exec.run_n(1)``.
   3. Read the log vector.

   **Expected outcome.** ``log == vec![1, 2, 3]`` — order is
   preserved across the chain.

.. test:: Diamond DAG runs every vertex exactly once
   :id: TEST_0115
   :status: implemented
   :verifies: REQ_0021

   **Goal.** A four-vertex diamond
   (root → {left, right} → merge) under
   ``Executor::add_graph`` runs each vertex concurrently when its
   in-edges are satisfied and exactly once per triggering cycle.

   **Fixture.** ``crates/taktora-executor/tests/graph.rs`` —
   ``diamond_runs_all_vertices_once`` (lines 8-47). A two-worker
   executor; four vertices, each incrementing its own
   ``AtomicU32``; edges ``r→l``, ``r→rt``, ``l→m``, ``rt→m``;
   root ``r`` carries a 10 ms interval trigger.

   **Steps.**

   1. Build the graph via ``g.vertex(...).edge(r, l).edge(...).root(r)``.
   2. Call ``exec.run_n(1)``.
   3. Read each counter.

   **Expected outcome.** Every counter equals ``1`` — concurrent
   dispatch when in-edges are satisfied, gating until upstream
   vertices have completed.

.. test:: StopChain and Err propagate to downstream items
   :id: TEST_0116
   :status: implemented
   :verifies: REQ_0022

   **Goal.** An item returning ``Ok(ControlFlow::StopChain)`` or
   ``Err`` prevents downstream items in its enclosing chain or
   DAG from being dispatched within the same triggering cycle.

   **Fixture.** Three Rust tests cover the variants:
   ``crates/taktora-executor/tests/chain.rs`` —
   ``stop_chain_aborts_remaining_items`` (lines 45-77) and
   ``err_in_middle_propagates_and_stops`` (lines 79-104); plus
   ``crates/taktora-executor/tests/graph.rs`` —
   ``root_stop_chain_skips_dependents`` (lines 49-72).

   **Steps.**

   1. ``stop_chain_aborts_remaining_items``: head returns
      ``StopChain``; tail increments a counter. After
      ``run_n(1)`` assert ``counter == 1`` (tail did not run).
   2. ``err_in_middle_propagates_and_stops``: mid returns
      ``Err``; tail increments a counter. After ``run_n(1)``
      assert ``tail_seen == 0`` and ``run_n`` returns an error
      whose ``Display`` contains the original ``mid-err`` text.
   3. ``root_stop_chain_skips_dependents``: graph root returns
      ``StopChain``; leaf increments a counter. After
      ``run_n(1)`` assert ``leaf == 0``.

   **Expected outcome.** All three assertions hold — abort
   semantics propagate identically across the chain and graph
   dispatch paths.

.. test:: wrap_with_condition gates execution on the predicate
   :id: TEST_0117
   :status: implemented
   :verifies: REQ_0023

   **Goal.** ``wrap_with_condition(item, predicate)`` runs the
   wrapped item when ``predicate()`` is true and short-circuits
   when it is false.

   **Fixture.** In-source tests in
   ``crates/taktora-executor/src/condition.rs`` —
   ``condition_true_runs_inner`` (lines 67-73) and
   ``condition_false_stops_chain`` (lines 75-81). Each wraps an
   inner ``item`` returning ``Ok(Continue)`` and drives the
   wrapper through a ``ContextHarness``.

   **Steps.**

   1. ``condition_true_runs_inner``: wrap with ``|| true``;
      execute; assert the result is ``ControlFlow::Continue``.
   2. ``condition_false_stops_chain``: wrap with ``|| false``;
      execute; assert the result is ``ControlFlow::StopChain``.

   **Expected outcome.** Both branches behave as documented.
   Note that the false branch surfaces as ``StopChain`` and thus
   also short-circuits the surrounding chain — see the audit
   comment on REQ_0023.

----

Cycle-time watchdog
-------------------

Test cases verifying the cycle-time watchdog sub-feature
(:need:`FEAT_0014`).

.. test:: TriggerDeclarer::deadline stores the (listener, deadline) pair
   :id: TEST_0118
   :status: implemented
   :verifies: REQ_0030

   **Goal.** ``TriggerDeclarer::deadline(subscriber, deadline)``
   records a ``TriggerDecl::Deadline { listener, deadline }`` so
   that the dispatch path can later attach a deadline guard and
   surface the missed-deadline condition.

   **Fixture.** In-source test in
   ``crates/taktora-executor/src/trigger.rs`` —
   ``collects_deadline_decl`` (lines 176-187). A
   ``TriggerDeclarer`` constructed via ``new_test()``; a
   ``Subscriber`` built over a unique iceoryx2 service.

   **Steps.**

   1. Create the subscriber and capture its
      ``listener_handle``.
   2. Call ``d.deadline(&sub, Duration::from_millis(50))``.
   3. Pattern-match the first declaration as
      ``TriggerDecl::Deadline { listener, deadline }``.
   4. Assert the stored ``listener`` is ``Arc::ptr_eq`` to the
      expected handle and the stored ``deadline`` equals
      ``Duration::from_millis(50)``.

   **Expected outcome.** The declaration is recorded with both
   fields intact, which is the boundary the executor's
   ``WaitSet::attach_deadline`` consumes.

.. test:: ExecutionMonitor::post_execute reports per-execute duration
   :id: TEST_0119
   :status: implemented
   :verifies: REQ_0031

   **Goal.** ``ExecutionMonitor::post_execute(task, started_at,
   took, ok)`` is called once per dispatch with a non-zero
   ``took`` and the matching ``TaskId``.

   **Fixture.** ``crates/taktora-executor/tests/monitor.rs`` —
   ``monitor_brackets_each_execute`` (lines 26-48). The
   ``RecordingMonitor`` records each ``post_execute`` invocation
   as ``(TaskId, Duration, bool)`` in a ``Mutex<Vec<...>>``.

   **Steps.**

   1. Attach the monitor; register a cyclic item at 10 ms
      period.
   2. Call ``exec.run_n(3)``.
   3. Read the recorded vector.

   **Expected outcome.** Three ``(task, took, ok)`` triples are
   captured, all with ``ok == true``; the post-execute timing
   signature is wired through to every dispatch path.

----

Real-time scheduling
--------------------

Test cases verifying the real-time worker scheduling sub-feature
(:need:`FEAT_0015`).

.. test:: ThreadAttributes affinity_mask compiles and runs
   :id: TEST_0127
   :status: implemented
   :verifies: REQ_0040

   **Goal.** Under the ``thread_attrs`` cargo feature, a
   ``ThreadAttributes`` carrying an ``affinity_mask(vec![0])``
   passes through ``ExecutorBuilder::worker_attrs`` and the pool
   workers apply it via ``core_affinity::set_for_current``
   without panicking or failing to dispatch.

   **Fixture.** ``crates/taktora-executor/tests/thread_attrs.rs``
   — ``worker_attrs_compiles_and_runs`` (lines 7-29). A
   two-worker executor with attributes
   ``name_prefix("taktora-test")`` and
   ``affinity_mask(vec![0])``; one cyclic item at 10 ms period.

   **Steps.**

   1. Build the attributes; build the executor with
      ``worker_attrs(attrs)``.
   2. Register the cyclic item.
   3. Call ``exec.run_n(1)``.

   **Expected outcome.** The run completes without error; the
   affinity application path is exercised end-to-end on the
   feature-gated build.

.. test:: ThreadAttributes priority setter compiles into the worker thread body
   :id: TEST_0128
   :status: implemented
   :verifies: REQ_0041

   **Goal.** Under the ``thread_attrs`` cargo feature on
   ``target_os = "linux"``, the ``ThreadAttributes::priority``
   setter and the worker thread's ``set_sched_fifo`` call site
   compile and run as part of the worker startup path. Behavioural
   verification of the ``SCHED_FIFO`` policy actually taking
   effect requires ``CAP_SYS_NICE``, which the test host typically
   lacks; ``set_sched_fifo`` swallows the ``EPERM`` and the worker
   continues under ``SCHED_OTHER`` — see the audit note on
   REQ_0041.

   **Fixture.** ``crates/taktora-executor/tests/thread_attrs.rs``
   — ``worker_attrs_compiles_and_runs`` (lines 7-29) exercises
   the ``worker_attrs`` path on which ``set_sched_fifo`` is
   conditionally called.

   **Steps.**

   1. Build a ``ThreadAttributes`` value (the type carries the
      ``priority`` field even when the test does not set it).
   2. Build the executor with the attributes; register one
      cyclic item.
   3. Call ``exec.run_n(1)``.

   **Expected outcome.** The build, executor construction, and
   dispatch all succeed — exercising the compile-time presence
   of the ``priority`` field and the
   ``set_sched_fifo`` call site. Mechanical verification that
   the policy is honoured requires running the host with
   ``CAP_SYS_NICE`` (out of scope for unprivileged CI).

----

Cooperative shutdown
--------------------

Test cases verifying the cooperative shutdown sub-feature
(:need:`FEAT_0016`).

.. test:: Stoppable::stop wakes an idle WaitSet from another thread
   :id: TEST_0129
   :status: implemented
   :verifies: REQ_0051

   **Goal.** A ``Stoppable`` handle cloned *before* ``run()`` —
   which is wired to the executor's notifier at ``build()`` time
   — wakes the WaitSet within a bounded time when ``stop()`` is
   called from another thread, even when the only registered
   trigger is a 60-second interval.

   **Fixture.** ``crates/taktora-executor/tests/stoppable.rs`` —
   ``stop_from_other_thread_wakes_idle_executor`` (lines 12-45).
   An inline-pool executor with one item carrying a 60-second
   interval trigger; a ``Stoppable`` clone obtained before
   ``run()``; a helper thread that sleeps 50 ms then calls
   ``stop()``.

   **Steps.**

   1. Build the executor and register the long-interval item.
   2. Clone ``exec.stoppable()``.
   3. Spawn the helper thread that calls ``stop()`` after
      50 ms.
   4. Call ``exec.run()`` and measure the elapsed time.

   **Expected outcome.** ``run()`` returns in under 2 s — the
   stop notifier wakes the WaitSet promptly rather than
   blocking on the 60-second interval. Quantifies "bounded
   time" empirically at the 2 s ceiling.

----

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

----

Cycle-overrun fault primitive
-----------------------------

Tests verifying the cycle-overrun fault primitive sub-feature
(:need:`FEAT_0018`).

.. test:: Budget breach faults task and halts dispatch
   :id: TEST_0815
   :status: open
   :verifies: REQ_0070, REQ_0102

   Item with :code:`interval(5ms); budget(1ms);` sleeps 3ms in
   :code:`execute()`. After one wakeup: task state is
   :code:`Faulted{BudgetExceeded}`, :code:`overrun_count >= 1`,
   :code:`Observer::on_task_fault` fired exactly once. Subsequent
   wakeups must NOT invoke :code:`execute()` again.

.. test:: Clear task fault resumes dispatch
   :id: TEST_0816
   :status: open
   :verifies: REQ_0070

   After the task is Faulted (per :need:`TEST_0815`),
   :code:`clear_task_fault` transitions back to Running. Subsequent
   wakeups invoke :code:`execute()` again.
   :code:`Observer::on_task_clear` fires exactly once. A second breach
   re-fires the full cycle.

.. test:: Iteration budget faults executor with silent cascade
   :id: TEST_0817
   :status: open
   :verifies: REQ_0071, REQ_0073

   Executor with :code:`iteration_budget(10ms)`. Two items registered:
   one healthy, one breaching. After one breach: executor state is
   :code:`Faulted{IterationBudgetExceeded}`,
   :code:`Observer::on_executor_fault` fired once, and the healthy
   item transitioned to :code:`Faulted{ExecutorFaulted}` WITHOUT
   per-task :code:`on_task_fault` firing.
   :code:`clear_executor_fault` cascade-clears both, firing
   :code:`on_executor_clear` once and :code:`on_task_clear` per
   cleared task.

.. test:: Fault handler dispatches in place of main item
   :id: TEST_0818
   :status: open
   :verifies: REQ_0072

   Item registered via
   :code:`add_with_fault_handler(main, handler)`. After :code:`main`
   breaches budget: subsequent wakeups invoke :code:`handler.execute()`,
   not :code:`main.execute()`. :code:`clear_task_fault` restores main
   dispatch.

.. test:: Overrun count persists across clears
   :id: TEST_0819
   :status: open
   :verifies: REQ_0102

   Force a breach, clear, force another breach.
   :code:`overrun_count` is monotonic; not reset by
   :code:`clear_task_fault`.

.. test:: Fault state set from worker visible from main
   :id: TEST_0820
   :status: open
   :verifies: REQ_0073

   Multi-worker setup; per-task fault state and
   :code:`overrun_count` set from a pool worker thread are visible
   to the main thread without torn reads or panics.

.. test:: Overrun post-execute path zero allocations
   :id: TEST_0821
   :status: open
   :verifies: REQ_0060, REQ_0104

   :code:`CountingAllocator` tracks the steady-state overrun post-execute
   path via the differential-measurement pattern (large vs small
   :code:`run_n`); per-iteration allocs == 0.

.. test:: Fault callbacks forwarded to tracing
   :id: TEST_0822
   :status: open
   :verifies: REQ_0073

   :code:`taktora-executor-tracing`'s :code:`TracingObserver` forwards
   :code:`on_task_fault`, :code:`on_task_clear`, :code:`on_executor_fault`,
   :code:`on_executor_clear` to :code:`tracing::warn!` /
   :code:`tracing::info!` on target :code:`taktora.fault` with the
   documented field shape.
