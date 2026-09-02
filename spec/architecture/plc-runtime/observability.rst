Scan-cycle observability
========================

Detailed design for the **scan-cycle observability** sub-feature
(:need:`FEAT_0021`). Two structural pieces: a fixed-bucket histogram
for percentile estimation (chosen for its allocation-free, bounded-time
per-sample update path), and per-task aggregate slots allocated at
``Executor::build`` time.

Each completed (or faulted) scan cycle folds one observation into the
per-task statistics through an allocation-free update path, then publishes
both a raw push sample and an aggregated pull snapshot:

.. mermaid::

   flowchart LR
       Pre["pre_execute<br/>(task-logic start, telemetry clock)"]
       Post["post_execute<br/>(took / actual_period / jitter / lateness)"]
       subgraph Update["allocation-free per-sample update (REQ_0104)"]
           Hist["Histogram.record(took_ns)<br/>fixed sub-octave buckets (ADR_0060, REQ_0852)"]
           Deque["MinMaxDeque.record(took)<br/>exact windowed min/max (REQ_0105)"]
           Jit["max_jitter_ns (REQ_0101)<br/>max_lateness_ns (REQ_0106)"]
           Ovr["overrun_count (REQ_0102)"]
       end
       Push["on_cycle_stats(&CycleObservation)<br/>raw sample, once per scan attempt (REQ_0103)"]
       Pull["Executor::stats_snapshot()<br/>p50 / p95 / p99 · min / max ·<br/>max_jitter · max_lateness · overrun"]
       Gate["exact-extreme SLO gate (REQ_0851)<br/>pass/fail uses exact extremes, not buckets"]

       Pre --> Post
       Post --> Hist
       Post --> Deque
       Post --> Jit
       Post --> Ovr
       Post --> Push
       Hist --> Pull
       Deque --> Pull
       Jit --> Pull
       Ovr --> Pull
       Deque --> Gate
       Jit --> Gate

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
   (≈ 37 kB / task: the sub-octave histogram is ``BUCKETS · 4 B`` ≈ 9 kB
   per segment × 4 segments, plus snapshots — see :need:`REQ_0852`).
   ❌ Percentile values are bucket-quantised. The shipped sub-octave layout
   (:need:`REQ_0852`) bounds the geometric-midpoint estimate at ≤ 1 %
   (``taktora_stats::PERCENTILE_MAX_REL_ERR_PCT``) across 100 ns … 10 s —
   tightened from the original octave layout's ``√2`` (≈ +42 % / −29 %).
   Even so, the histogram is trend telemetry: any threshold/SLA decision
   uses the exact-extreme gate of :need:`REQ_0851`, and the :need:`REQ_0111`
   harness exposes raw samples for exact offline percentiles.

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
   :status: implemented
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
   :status: implemented
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
   :status: implemented
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
     actual_period_ns, jitter_ns, lateness_ns, took_ns, skipped_slots }``,
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
     ``took``, ``jitter``, and ``lateness`` (grid: scan count +
     dispatcher skip signal per :need:`ADR_0101`) into the task's
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
   :status: implemented
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
