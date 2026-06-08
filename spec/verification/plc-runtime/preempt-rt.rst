PREEMPT_RT validation harness
=============================

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
