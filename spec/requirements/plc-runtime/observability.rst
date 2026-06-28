Scan-cycle observability
========================

Gap capability: first-class statistics on cycle-time behaviour —
percentiles, jitter, overrun counts — exposed without requiring users to
build their own.

.. feat:: Scan-cycle observability
   :id: FEAT_0021
   :status: open
   :satisfies: FEAT_0010

   First-class statistics on cycle-time behaviour — percentiles, jitter,
   overrun counts — exposed without requiring users to build their own.

.. req:: Per-task latency percentiles
   :id: REQ_0100
   :status: implemented
   :satisfies: FEAT_0021
   :links: BB_0050, IMPL_0070, TEST_0190

   The runtime shall report p50, p95, and p99 execute-duration percentiles
   per registered task, computed over a sliding window whose size is
   configurable at ``Executor::build`` time.

   Percentile estimation uses a fixed, compile-time bucket histogram so the
   per-sample update path is allocation-free (:need:`REQ_0104`,
   :need:`ADR_0060`). The reported percentile is the **geometric midpoint**
   of the occupied bucket, which removes the systematic downward bias of a
   lower-edge estimate and bounds the estimate's relative error at
   ``taktora_stats::PERCENTILE_MAX_REL_ERR_PCT``. The shipped sub-octave
   bucket layout (:need:`REQ_0852`) holds this to ≤ 1 % across the
   100 ns … 10 s range.

   Even at ≤ 1 %, these percentiles remain **telemetry for trend and shape;
   any SLA, acceptance, commissioning, or regression decision shall instead
   use the exact windowed extremes of the exact-extreme SLO conformance gate
   (:need:`REQ_0851`).** The histogram is deliberately *not* authoritative
   for pass/fail.

.. req:: Exact-extreme SLO conformance gate
   :id: REQ_0851
   :status: implemented
   :satisfies: FEAT_0021
   :links: BB_0050, IMPL_0070, TEST_0849, TEST_0191, TEST_0850

   Any pass/fail decision on cycle-time behaviour — acceptance test,
   commissioning sign-off, regression gate, or alarm threshold — shall be
   evaluated against the **exact** windowed extremes the runtime retains,
   never against the bucket-quantised percentiles of :need:`REQ_0100`. The
   percentile histogram carries up to
   ``taktora_stats::PERCENTILE_MAX_REL_ERR_PCT`` relative error by
   construction and is reserved for trend visualisation.

   The authoritative quantities, each an actual observed sample carrying no
   bucket-quantisation error and aged with the same sliding window as
   :need:`REQ_0100`, are:

   * **execute-duration min/max** — :need:`REQ_0105` (monotonic-deque
     backed, exact);
   * **maximum period jitter** — :need:`REQ_0101` (exact windowed maximum);
   * **maximum deadline lateness** — :need:`REQ_0106` (exact signed windowed
     maximum).

   All three are already exposed on both the pull snapshot and the push /
   NDJSON paths (:need:`REQ_0103`, :need:`REQ_0111`). A jitter SLO of the
   form "max jitter ≤ X over the window" is therefore decidable **exactly,
   today**, with no dependency on the histogram-precision work of
   :need:`REQ_0852`. This requirement records the policy that the exact
   path — not the estimate — is the gate.

.. req:: Sub-octave percentile precision
   :id: REQ_0852
   :status: implemented
   :satisfies: FEAT_0021
   :refines: REQ_0100
   :links: BB_0053, TEST_0868

   To make the percentile **estimate** of :need:`REQ_0100` itself
   threshold-grade, the histogram shall bound percentile relative error at
   ≤ 1 % at bucket centroids across the range 100 ns … 10 s, while
   preserving the allocation-free, bounded-time per-sample update of
   :need:`REQ_0104`. This requires a sub-octave (log-linear /
   mantissa-subdivided) bucket layout in place of the shipped octave
   buckets. Verified by :need:`TEST_0868`.

   **Realisation.** ``taktora-stats`` subdivides each octave linearly into
   ``M = 64`` sub-buckets (``BUCKETS = 2304`` over octaves 0…35), keeping
   ``bucket_index`` integer-only and O(1)
   (``sub = ((v − 2^o)·M) >> o``). Measured worst-case error on the
   :need:`TEST_0868` reference distributions is 0.41 %. Linear (rather than
   geometric) subdivision is what keeps the hot path branch- and
   float-free; it costs more buckets than the ~115/decade geometric floor
   noted below, raising the histogram footprint to ``BUCKETS · 4 B`` per
   segment (≈ 9 kB; ≈ 37 kB/task at the default 4 segments). The estimate
   is now ≤ 1 %, but the authoritative pass/fail path remains the
   exact-extreme gate of :need:`REQ_0851`.

   **Math note.** The ≤ 1 % bound and "≥ 3 buckets per decade" are
   *independent* constraints, not equivalent ones. Three buckets per decade
   give ~factor-2 bucket width (≈ 40 % worst-case intra-bucket error); a
   ≤ 1 % centroid bound needs bucket edges in roughly a 1.02 ratio
   (~115 buckets per decade). The superseded wording of :need:`REQ_0100`
   and the original :need:`ADR_0060` consequence note conflated the two.

.. req:: Per-task maximum jitter
   :id: REQ_0101
   :status: implemented
   :satisfies: FEAT_0021
   :links: BB_0050, IMPL_0070, TEST_0191

   The runtime shall report the maximum observed jitter — defined as the
   absolute difference between actual and declared scan period — per
   cyclic task, computed over the same sliding window as
   :need:`REQ_0100`. Lifetime maxima are out of scope; the reported value
   ages out with the window.

.. req:: Per-task overrun counter
   :id: REQ_0102
   :status: implemented
   :satisfies: FEAT_0021
   :refines: REQ_0070
   :links: BB_0093, IMPL_0081, TEST_0815, TEST_0819

   The runtime shall expose a monotonic counter per task that increments
   on each scan-cycle execution that exceeds the declared budget per
   :need:`REQ_0070`. The counter shall not reset on
   :code:`Executor::clear_task_fault`; it tracks lifetime breaches.

.. req:: Statistics query API
   :id: REQ_0103
   :status: implemented
   :satisfies: FEAT_0021
   :links: BB_0051, IMPL_0070, TEST_0193

   Cycle-cycle statistics shall be available via two distinct paths:

   * **Push** — the ``Observer`` trait shall expose an
     ``on_cycle_stats(&CycleObservation)`` callback (provided as a no-op
     default for backward compatibility) that fires once per scan cycle
     (including a faulted scan, see :need:`REQ_0107`) with the raw
     per-cycle observation (``cycle_index``, ``task_id``, ``task_index``,
     ``faulted``, ``period_ns``, ``pre_ns``, ``actual_period_ns``,
     ``jitter_ns``, ``lateness_ns``, ``took_ns``, ``skipped_slots``). The ``cycle_index`` is
     the monotonic scan count of :need:`REQ_0107`, the join key by which a
     cyclic connector's telemetry (:need:`REQ_0265`) composes with the
     executor's. The push path delivers raw samples, not aggregates.

     The observation shall additionally carry ``task_index`` — the task's
     stable zero-based registration index — as a flat ``u32`` identity/join
     key (so a consumer need not hash the ``Arc<str>`` ``task_id`` on the hot
     path) and ``pre_ns`` — the telemetry-clock nanosecond instant of
     task-logic start (the canonical reference point of :need:`REQ_0101`).
     ``pre_ns`` is the single time source for an exported sample's time axis;
     a consumer shall not read a second clock. Both fields are always present
     (never absent), including on a faulted scan.
     ``skipped_slots`` — the dispatcher skip count of :need:`REQ_0840` —
     is likewise always present (``0`` when nothing was skipped),
     including on a faulted scan.

     A faulted scan shall be **distinguishable** from a healthy one, the
     cross-layer twin of the connector's ``CycleOutcome`` (:need:`REQ_0267`):
     the observation shall carry a ``faulted`` flag, and every measured
     quantity (``actual_period_ns``, ``jitter_ns``, ``lateness_ns``,
     ``took_ns``) shall encode "not measured this cycle" as *absent*
     (``Option::None``), never as a measured ``0`` — so a consumer joining
     the executor and connector push streams on ``cycle_index`` sees a
     consistent absent-on-fault signal from both layers rather than an
     ambiguous zero.
   * **Pull** — ``Executor::stats_snapshot()`` shall return a borrowed
     view of the current per-task aggregates (``p50``, ``p95``, ``p99``,
     ``max_jitter_ns``, ``overrun_count``), readable concurrently with
     dispatch.

   Both paths shall be allocation-free on the runtime side (see
   :need:`REQ_0104`); allocations on the consumer side are out of scope.

.. req:: Allocation-free telemetry update
   :id: REQ_0104
   :status: implemented
   :satisfies: FEAT_0021
   :refines: REQ_0060
   :links: BB_0053, IMPL_0070, TEST_0194

   The runtime's per-sample telemetry update path — the code that runs
   inside the dispatch loop's timing hooks to update the histogram,
   max-jitter, and overrun counter — shall perform zero heap allocations
   and shall complete in bounded time.

   The update path's worst-case runtime shall be dominated by the
   histogram bucket-index computation (a ``log2``-style lookup, no loops
   over samples) and atomic updates to the bucket counter plus the
   max-jitter and overrun fields. The verification harness mirrors
   :need:`TEST_0170` (``CountingAllocator`` covering pool worker
   threads); see :need:`TEST_0194`.

.. req:: Per-task exact min/max execute duration
   :id: REQ_0105
   :status: implemented
   :satisfies: FEAT_0021
   :links: BB_0050, IMPL_0070, TEST_0849

   In addition to the bucket-quantised percentiles of :need:`REQ_0100`,
   the runtime shall report the **exact** minimum and maximum
   execute-duration observed per registered task, over the same sliding
   window as :need:`REQ_0100`. "Exact" means the reported values are
   actual observed samples, not bucket centroids — the absolute
   worst-case sample is retained, not merely the top occupied bucket.

   The min/max shall age out with the window (lifetime extrema are out of
   scope, consistent with :need:`REQ_0101`). The implementation shall be
   allocation-free per :need:`REQ_0104`; a fixed-capacity monotonic deque
   (sized to the window at ``Executor::build`` time) is the intended
   mechanism, since the histogram of :need:`ADR_0060` cannot recover an
   exact extremum after ageing-out by snapshot subtraction.

.. req:: Per-task deadline lateness
   :id: REQ_0106
   :status: implemented
   :satisfies: FEAT_0021
   :links: BB_0050, IMPL_0070, TEST_0850, TEST_0856

   For each cyclic task, the runtime shall report **deadline lateness** —
   the signed offset between the task's actual task-logic start (the
   ``pre_execute`` instant) and the nominal periodic grid point at which
   it was due to start — over the same sliding window as
   :need:`REQ_0100`. Positive lateness means the task started late.

   Deadline lateness is distinct from the period jitter of
   :need:`REQ_0101`: jitter captures the spread of the measured period
   and is blind to a constant offset, whereas lateness captures steady
   drift or constant offset from the grid. The reported aggregate shall
   include at least the windowed maximum (most-late) lateness; the raw
   per-cycle ``lateness_ns`` is delivered on the push path of
   :need:`REQ_0103`. Event-driven (non-cyclic) tasks have no declared
   period and therefore report no lateness.

   **Grid anchoring.** The nominal grid point for a cycle is
   ``grid_epoch + grid_slot × period``, where ``grid_epoch`` is the
   task's **own** first recorded dispatch **back-dated by the
   dispatcher's one-shot ``late_by`` signal** — the distance from that
   dispatch to the most recent dispatcher grid point at or before it —
   so the grid anchors at the first dispatch's **nominal slot** and a
   late process start is reported once, honestly, as first-cycle
   lateness instead of becoming a permanent negative floor on every
   later on-grid cycle (a faulted first scan anchors too — its dispatch
   instant is real). ``grid_slot`` advances by
   exactly **one slot per scan attempt plus the dispatcher's
   skipped-slot signal** (:need:`REQ_0840`). The slot is never
   reconstructed from the measured period: rounding
   ``actual_period / period`` over-counts on a coalesced catch-up wake
   (a late cycle followed by a short catch-up cycle), stamping a
   permanent spurious negative step into every later cycle's lateness
   (:need:`ADR_0101`). A steady sub-period slip therefore accumulates
   as intended; a late-but-served wake shows a transient positive spike
   that heals on the catch-up dispatch; a dispatcher skip-realign
   (:need:`REQ_0268`) re-anchors through the signal. Absent a signal —
   the ``Legacy`` relative-timer mode never skips slots — a whole
   missed period honestly remains visible as a persistent offset rather
   than being silently absorbed. Absent the anchor signal likewise —
   ``Legacy`` mode and event-driven dispatch have no dispatcher grid —
   the epoch stays the first recorded dispatch itself, and a constant
   phase offset already present there reads as zero by construction.
   The slot advance is aligned with the
   :need:`REQ_0107` ``cycle_index`` (one per scan attempt) except for
   the explicit skip signal.

.. req:: Per-task scan index and faulted-scan emission
   :id: REQ_0107
   :status: implemented
   :satisfies: FEAT_0021
   :links: IMPL_0070, TEST_0851

   The runtime shall maintain, per cyclic task, a monotonic zero-indexed
   ``cycle_index`` (scan count) incremented once per scan **attempt**, and
   shall include it in the push observation of :need:`REQ_0103`. The
   runtime shall fire ``on_cycle_stats`` and increment ``cycle_index`` on
   **every** scan attempt, including a scan whose task logic returned an
   error or was otherwise faulted — not only completed scans.

   This exists so the executor's telemetry composes with a cyclic
   connector's (:need:`FEAT_0038`): because the NC task fires exactly once
   per bus cycle (one-network-one-process), the executor's per-task
   ``cycle_index`` equals the connector's per-cycle ``cycle_index``
   (:need:`REQ_0265`, :need:`REQ_0267`) for the same cycle, giving a
   consumer an explicit join key. Were the executor to skip a faulted
   scan, its count would lag the connector's from the first fault onward
   and every downstream pairing would desync — so emit-on-fault is
   required symmetrically on both layers. The update is allocation-free
   per :need:`REQ_0104`.

.. req:: Per-task skipped-slot count
   :id: REQ_0840
   :status: implemented
   :satisfies: FEAT_0021
   :links: IMPL_0070, TEST_0853

   For each cyclic task, the runtime shall report per scan cycle the
   number of nominal grid slots the dispatcher passed over **unserved**
   between the slot served by the task's previous dispatch and the slot
   served by this dispatch (the skip-realign of :need:`REQ_0268`). The
   count shall be ``0`` in steady state, ``0`` whenever the dispatch
   mode provides no skip signal (the ``Legacy`` relative timer), and
   ``0`` on a task's first recorded cycle — the lateness grid of
   :need:`REQ_0106` anchors there, so earlier slots do not exist on the
   task's own grid.

   The count shall be carried on the push observation of
   :need:`REQ_0103` (field ``skipped_slots``, always present, never
   ``null``) and on the NDJSON record of :need:`REQ_0111`. The lateness
   grid of :need:`REQ_0106` shall advance by exactly
   ``1 + skipped_slots`` per scan attempt, so a dispatcher skip
   re-anchors the grid without per-cycle reconstruction. Only this
   discrete count crosses the scheduler→telemetry boundary; every
   timestamp remains on the telemetry measurement clock
   (:need:`REQ_0268`). A skipped slot is a scan that never executed —
   previously invisible in the exported stream. The update is
   allocation-free per :need:`REQ_0104`.
