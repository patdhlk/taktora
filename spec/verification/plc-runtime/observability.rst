Scan-cycle observability
========================

Test cases verifying the scan-cycle observability sub-feature
(:need:`FEAT_0021`). This area also carries the witnesses for the
absolute-grid cyclic dispatch (:need:`REQ_0268`), the skip-signal
re-anchoring (:need:`REQ_0840`), the EINTR-immune run loop
(:need:`REQ_0269`), and the tight dispatch-thread timer slack
(:need:`REQ_0274`), since they share the per-cycle telemetry harness.

.. test:: Histogram percentile accuracy
   :id: TEST_0190
   :status: implemented
   :verifies: REQ_0100

   **Goal.** Confirm the :need:`ADR_0060` histogram returns p50, p95,
   p99 estimates within the *documented* relative-error bound
   (``taktora_stats::PERCENTILE_MAX_REL_ERR_PCT``) when fed a known
   reference distribution — i.e. that the geometric-midpoint estimate is
   bounded as specified, not that it is exact. The ≤ 1 % accuracy target is
   verified separately by :need:`TEST_0868`.

   **Fixture.** A standalone unit test in
   ``crates/taktora-stats/src/histogram.rs`` that drives
   ``RollingHistogram`` directly (no full executor), with a deterministic
   sample generator (no clock, no ``rand``).

   **Steps.**

   1. Build a ``RollingHistogram`` with the production ``BUCKETS`` layout
      and a window sized to hold the full sample set.
   2. Feed it 10 000 samples drawn from a known distribution
      (uniform on ``[100 ns, 100 ms]`` and exponential with mean
      ``1 ms``).
   3. Compute exact percentile values from the input samples and
      compare to ``RollingHistogram::percentile(q)`` for q ∈ {500, 950,
      990} permille.
   4. Assert relative error ≤ ``PERCENTILE_MAX_REL_ERR_PCT`` for each
      percentile in each distribution.

   **Expected outcome.** All six assertions hold (3 quantiles × 2
   distributions).

   Lives under
   ``crates/taktora-stats/src/histogram.rs`` ``#[cfg(test)]``.

.. test:: Sub-octave percentile accuracy (≤ 1 %)
   :id: TEST_0868
   :status: implemented
   :verifies: REQ_0852

   **Goal.** Confirm the refined sub-octave histogram returns p50, p95,
   p99 within ≤ 1 % relative error at bucket centroids on a known
   reference distribution.

   **Fixture / steps.** As :need:`TEST_0190`, but assert relative error
   ≤ 1 % (a literal bound) rather than ``PERCENTILE_MAX_REL_ERR_PCT``.
   Realised as ``sub_octave_percentile_accuracy_within_one_percent`` in
   ``crates/taktora-stats/src/histogram.rs``; measured worst-case error is
   0.41 % across the six assertions.

.. test:: Per-task max jitter under synthetic period violation
   :id: TEST_0191
   :status: implemented
   :verifies: REQ_0101

   **Goal.** A period violation produces the exact max-jitter readout.

   **Fixture.** Executor with one cyclic task whose telemetry clock is an
   injected ``MockClock`` (``ExecutorBuilder::clock``). The task body
   advances the mock clock to *simulate* each cycle's spacing, so the
   measured period — and therefore jitter — is independent of the host
   scheduler. The real interval only paces wakeups; no wall-clock timing
   enters the assertion.

   **Steps.**

   1. Build executor with a ``MockClock``; register a cyclic task whose
      nominal period equals the body's baseline advance (jitter 0).
   2. Run 60 cycles where every 5th cycle advances the mock clock by an
      extra ``DELTA`` (5 ms), inducing an exact period overshoot.
   3. Query ``Executor::stats_snapshot``; read
      ``per_task[0].max_jitter_ns``.
   4. Assert ``max_jitter_ns == DELTA`` exactly (equality, no tolerance
      band).

   **Expected outcome.** Max jitter equals the injected overshoot to the
   nanosecond.

   **Rationale for the mock clock.** Deriving jitter from a real
   ``Instant`` made the figure scheduler-dependent (a loaded CI runner
   inflated it to ~69 ms), forcing loose bounds that tested the runner
   rather than :need:`REQ_0101`. Scripting the telemetry clock removes the
   scheduler from the measurement and turns the band into an equality.
   The default ``SystemClock`` path is covered separately by the
   real-clock smoke test in
   ``crates/taktora-executor/tests/cycle_stats_real_clock_smoke.rs``.

   Lives under
   ``crates/taktora-executor/tests/cycle_stats_max_jitter.rs``.

.. test:: Overrun counter increments exactly per overrun cycle
   :id: TEST_0192
   :status: implemented
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
   :status: implemented
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
   :status: implemented
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

.. test:: Exact windowed min/max retain observed extremes
   :id: TEST_0849
   :status: implemented
   :verifies: REQ_0105

   **Goal.** The ``stats_snapshot`` ``min_ns``/``max_ns`` retain the
   exact observed execute-duration extremes, not bucket centroids.

   **Fixture.** Executor (``worker_threads(0)``) with one cyclic task
   whose telemetry clock is an injected ``MockClock``. Each cycle's
   ``took`` is the mock-clock delta across the body, so a body that
   advances the clock by a fixed amount yields a ``took`` of exactly that
   amount. The body advances by ``BASE`` (1 ms) normally and ``SPIKE``
   (20 ms) on exactly one cycle.

   **Steps.**

   1. Build executor with a ``MockClock``; register the cyclic task.
   2. Run 20 cycles, injecting the single ``SPIKE`` advance on one cycle.
   3. Read ``stats_snapshot().per_task[0]``.
   4. Assert ``max_ns == SPIKE`` and ``min_ns == BASE`` exactly. Equality
      at nanosecond precision can only arise from retaining the raw
      sample — an octave-bucket centroid (cf. ``p99_ns``) would land
      materially below ``SPIKE`` at this scale.
   5. Assert ``min_ns < max_ns`` (distinct extremes).

   **Expected outcome.** The exact extremes are retained to the
   nanosecond, distinct from the bucket-quantised percentiles. The earlier
   real-sleep version could only bound ``max_ns`` ∈ [18 ms, 30 ms]
   because a shared CI runner stretched the 20 ms sleep to ~87 ms; the
   mock clock makes the assertion an equality.

   Lives under
   ``crates/taktora-executor/tests/cycle_stats_minmax.rs``.

.. test:: Deadline lateness — drift accumulates, coalesced pair heals, offsets stay honest
   :id: TEST_0850
   :status: implemented
   :verifies: REQ_0106

   **Goal.** The scan-count-anchored lateness of :need:`REQ_0106`
   (1) **accumulates** a steady signed offset past one period (not a
   phase-within-period readout), (2) reports the issue-#46 coalesced
   catch-up pair as a single transient positive spike with **no
   fabricated negative lateness** and no permanent step, (3) absent a
   dispatcher skip signal, reports a whole missed period as an honest
   **persistent** offset, and (4) anchors each task's grid at its
   **own** first dispatch (in ``Legacy`` mode, the dispatch instant
   itself — no dispatcher grid exists; the ``Grid``-mode nominal-slot
   anchor is verified by :need:`TEST_0856`).

   **Fixture.** Executor (``worker_threads(0)``) with cyclic task(s);
   telemetry ``MockClock`` advanced from the task body; per-cycle
   lateness captured via the push ``Observer``. ``Legacy`` dispatch
   mode is forced: the scripted-clock figures must not depend on
   real-time dispatcher behavior (the ``Grid`` skip ferry is verified
   by :need:`TEST_0853`).

   **Steps — accumulation.**

   1. Every body advances the clock by ``PERIOD + DRIFT``
      (10 ms period, 2 ms slip, ``DRIFT < PERIOD/2``).
   2. Run 40 cycles; assert per-cycle lateness ``n × DRIFT`` and
      ``max_lateness_ns == 39 × DRIFT`` exactly.

   **Steps — coalesced catch-up pair (the #46 regression).**

   1. Every body advances ``PERIOD``, except cycle 10 advances
      ``1.6 × PERIOD`` (late wake) and cycle 11 advances
      ``0.4 × PERIOD`` (catch-up).
   2. Run 20 cycles.
   3. Assert per-cycle lateness is ``0`` everywhere except exactly
      ``+0.6 × PERIOD`` at the late cycle, and **no observation is
      negative** — the pre-fix reconstruction reported a permanent
      ``−PERIOD`` from cycle 12 onward.

   **Steps — missed period without a skip signal.**

   1. Every body advances ``PERIOD``, except one cycle advances
      ``2 × PERIOD``; no dispatcher skip is signalled (a mock-clock
      gap, not a real starvation).
   2. Run 25 cycles; assert every cycle from the gap onward reports
      exactly ``+PERIOD`` — the honest persistent offset. Healing
      requires the explicit :need:`REQ_0840` signal (see
      :need:`TEST_0853`).

   **Steps — per-task epoch.**

   1. Two cyclic tasks (10 ms and 20 ms) share the mock clock; only
      the 10 ms body advances it.
   2. Assert the first task's every sample is exactly ``0`` (it drives
      the clock itself) and the second task's **first** sample is
      exactly ``0`` — it anchors at its own first dispatch, where the
      pre-fix executor-shared epoch reported the start phase (at least
      one foreign period) from the first sample on. Later samples of
      the second task are not asserted: its scripted clock is driven by
      the first task's fire count, which is unbounded under real-time
      runner starvation; accumulation semantics are pinned by the
      single-task scenarios above.

   All four live in
   ``crates/taktora-executor/tests/cycle_stats_lateness.rs``.

.. test:: Cycle index is monotonic across faulted scans
   :id: TEST_0851
   :status: implemented
   :verifies: REQ_0107

   **Goal.** The per-task ``cycle_index`` is gap-free monotonic and
   ``on_cycle_stats`` fires on every scan attempt **including**
   faulted/fault-routed scans — the :need:`FEAT_0038` cross-layer join
   invariant.

   **Fixture.** Executor (``worker_threads(0)``) with one cyclic task at
   5 ms scan period and a 2 ms budget. The task body sleeps ~6 ms, which
   breaches the budget and faults on cycle 0, so the later wakeups are
   fault-routed. A custom ``Observer`` records each
   ``(cycle_index, took_ns)``.

   **Steps.**

   1. Build executor; register the cyclic task with a 5 ms period and a
      2 ms budget.
   2. Run 6 cycles.
   3. Assert exactly 6 ``on_cycle_stats`` emissions.
   4. Assert ``cycle_index`` is contiguous ``0..=5``.
   5. Assert cycle 0 reports ``took_ns > 0`` and the faulted cycles
      report ``took_ns == 0`` (poison-safe).

   **Expected outcome.** The ``cycle_index`` never lags across faults;
   the executor count stays equal to the connector's per-cycle index.

   Lives under
   ``crates/taktora-executor/tests/cycle_stats_faulted_scan.rs``.

.. test:: GridTimer holds the absolute grid (advance, skip-realign, multi-period)
   :id: TEST_0852
   :status: implemented
   :verifies: REQ_0268

   **Goal.** The pure ``GridTimer`` state machine
   (:need:`BB_0095`) phase-locks cyclic dispatch to an *absolute* grid —
   single-period advance accumulates zero offset, a stall skip-realigns
   without bursting, harmonic multi-period grids pick the earliest slot and
   coalesce coincident ones, and an empty grid yields no wakeup. These are
   the CI witnesses for the bounded long-run lateness of :need:`REQ_0268`;
   the long-run *hardware* drift bound is field evidence recorded in the Pi5
   A/B of :need:`ADR_0100`, not a CI test.

   **Fixture.** Deterministic unit tests over ``grid::GridTimer`` with an
   explicit ``now`` passed to ``next_timeout`` / ``take_due`` — **no clock,
   no sleep, no executor**. Tests live under
   ``crates/taktora-executor/src/grid.rs`` (``#[cfg(test)]`` module).

   **Steps.**

   1. *Single-period advance, zero offset.* With one cyclic period, drive
      ``take_due`` at successive ``now`` values that vary the per-cycle
      lateness; assert each slot fires exactly once and ``next_k`` stays on
      ``epoch + k × period`` — accumulated offset is zero regardless of how
      late the previous wakeup was.
   2. *Skip-realign on a stall.* Withhold ``take_due`` past one or more whole
      periods, then call it once; assert exactly **one** dispatch and that
      ``next_k`` snaps to the next *future* slot (closed-form re-anchor),
      never a replayed burst of stale cycles; a boundary-exact stall lands
      cleanly on a slot.
   3. *Harmonic multi-period.* With two harmonic periods, assert
      ``next_timeout`` returns the distance to the **earliest** pending slot
      and that slots coincident at a shared grid point coalesce within a
      single ``take_due`` pass.
   4. *Empty grid.* With no cyclic task registered, assert
      ``next_timeout(now) == Duration::MAX`` exactly (no grid-driven wakeup).

   **Expected outcome.** The grid never slides under per-cycle lateness, a
   transient stall costs bounded slots rather than a permanent phase offset,
   and multi-cadence grids share the one scheduling epoch — the structural
   guarantee behind the bounded lateness of :need:`REQ_0268`.

.. test:: Dispatcher skip-realign is carried, consumed once, and re-anchors lateness
   :id: TEST_0853
   :status: implemented
   :verifies: REQ_0840

   **Goal.** The :need:`REQ_0268` skip-realign reaches telemetry as
   ``skipped_slots`` exactly once, on the dispatch **after** the realign
   (backward-looking), advancing the lateness grid by ``1 + skipped`` so
   post-skip cycles read back on-grid.

   **Layer 1 — ``GridTimer`` unit (pure, exact).**

   1. Period 1000 ns, epoch 0; an on-grid dispatch carries
      ``skipped = 0``.
   2. Starve to 3500: the wake serves the overdue slot (carrying ``0`` —
      nothing was passed over before it), realigns to 4000, and records
      carry ``(4000 − 3000) / 1000 = 1``.
   3. The next ``take_due`` at 4000 carries ``skipped = 1``; at 5000 it
      is ``0`` again (consumed exactly once).
   4. A realign on a task's **first-ever** dispatch sets no carry —
      slots before the first dispatch do not exist on the task's own
      grid. Back-to-back realigns hand the carry over without loss or
      doubling.

   **Layer 2 — executor integration (Linux-only, real clocks, loose bounds).**

   1. ``Grid`` mode forced; one 50 ms cyclic task whose body sleeps
      ≈ 130 ms on one cycle (``worker_threads(0)`` — the dispatch
      thread genuinely starves).
   2. Run 8 wakeups capturing ``(lateness_ns, skipped_slots)`` via the
      push ``Observer`` (observation count is a lower bound — the
      non-Linux ``Grid`` fallback may emit fewer observations than
      wakeups).
   3. Assert: some observation reports ``skipped_slots ≥ 1``; the first
      cycle reports ``0``; the starved cycle's lateness spikes past one
      period; the final cycle's lateness is back under half a period
      (re-anchored, not accumulating).

   Layer 2 runs on Linux only — the production ``timerfd`` grid path.
   The non-Linux ``Grid`` fallback is not a real-time target: a stalled
   runner can leave the final sample mid-starvation, so no tail bound
   holds there; the carry mechanics remain covered everywhere by layer 1.

   Layer 1 lives in ``crates/taktora-executor/src/grid.rs`` (unit
   tests); layer 2 in
   ``crates/taktora-executor/tests/cycle_stats_skip_signal.rs``.

.. test:: Grid lateness anchors at the first dispatch's nominal slot
   :id: TEST_0856
   :status: implemented
   :verifies: REQ_0106

   **Goal.** In ``Grid`` mode a late first dispatch reports its real
   startup delay as first-cycle lateness — the epoch back-dates to the
   nominal slot — instead of erasing it to 0 and reading every later
   on-grid cycle as constantly early (the Pi5 −110 µs…−792 µs floor
   under loaded starts).

   **Fixture.** Linux-gated (the scripted call sequence is only
   deterministic on the production ``timerfd`` path): a scripted
   ``CyclicClock`` reads 0 at loop entry (grid epoch), 1.7 ms at the
   first wake (0.7 ms past the task's first nominal slot at 1 ms) and
   2.0 ms at the second; the telemetry clock is real; per-cycle
   lateness is captured via the push ``Observer``.

   **Steps.**

   1. ``run_n(2)`` with a 1 ms cyclic task.
   2. Assert sample 0's ``lateness_ns`` is **exactly** ``700 000`` —
      the anchor back-dates the first observed ``pre`` by the scripted
      ``late_by``, so no real-clock term enters the assertion (pre-fix:
      exactly ``0``).
   3. Assert no skip was signalled on either sample.

   Lives in ``crates/taktora-executor/tests/cycle_stats_grid_anchor.rs``.
   The exact ``late_by`` arithmetic — normal and snap paths, and the
   last-*passed*-lattice-point rule on a whole-slot first-dispatch miss
   — is pinned by the ``GridTimer`` unit tests in
   ``crates/taktora-executor/src/grid.rs``.

.. test:: Run loop survives EINTR storms
   :id: TEST_0854
   :status: implemented
   :verifies: REQ_0269

   **Goal.** A storm of handled signals interrupting the blocking wait
   neither terminates ``run_n`` nor skips or over-counts iterations.

   **Fixture.** Unix-gated: a no-op ``SIGUSR1`` handler (registered
   without ``SA_RESTART``), the dispatch loop on its own thread
   (``worker_threads(0)``, 5 ms cyclic task), and ``pthread_kill``
   pelting that exact thread every 10 ms across the whole nominal run
   window.

   **Steps.**

   1. ``run_n(50)`` while 25 signals land on the waiting thread.
   2. Assert ``run_n`` returns ``Ok`` **and** the task body ran exactly
      50 times (pre-fix: the first ``EINTR`` ended the run — observed
      1 of 50).

   Lives in ``crates/taktora-executor/tests/run_loop.rs``. Secondary
   guard: the ``preempt-rt`` bench warns on stderr when it wrote fewer
   records than the requested cycles, so a gracefully-truncated
   envelope (``SIGINT``/``SIGTERM``) cannot be mistaken for a complete
   run (``xtask/preempt-rt/tests/idle_bench.rs``).

.. test:: Dispatch thread runs with 1 µs timer slack
   :id: TEST_0855
   :status: implemented
   :verifies: REQ_0274

   **Goal.** The dispatch thread's effective timer slack is 1 µs, not
   the kernel's 50 µs ``SCHED_OTHER`` default.

   **Fixture.** Linux-gated: ``worker_threads(0)`` so the task body
   runs **on** the dispatch thread; the body reads its own
   ``/proc/<tid>/timerslack_ns`` (the file exists only at the top
   ``/proc/<pid>/`` level, which resolves for any thread id).

   **Steps.**

   1. ``run_n(1)``; assert the value read is exactly ``1000``.

   Lives in
   ``crates/taktora-executor/tests/dispatch_thread_timerslack.rs``; the
   drift impact (56.0 → 5.5 µs/cycle on the Pi5 rig) is recorded as
   field evidence on :need:`REQ_0274`.
