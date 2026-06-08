Connector cycle telemetry
=========================

Detailed design for the **connector cycle telemetry** sub-feature
(:need:`FEAT_0038`). The split between connector-measured and
executor-measured quantities is the central decision; the building
blocks reuse the shared ``taktora-stats`` primitive (:need:`ADR_0062`).

.. arch-decision:: Hybrid two-layer timing measurement
   :id: ADR_0063
   :status: open
   :refines: REQ_0265

   **Context.** Full timing visibility for a cyclic motion deployment
   spans two layers: the executor (when did the NC task fire, how long
   did its logic take) and the connector (how long was the wire round,
   did every device participate, were inputs fresh). A single layer
   cannot measure all of it. Critically, ``taktora-cyclic-fieldbus`` is
   ``#![no_std]`` with **no clock** — it can time a wire round only if
   the bus driver hands it a duration, and it cannot timestamp absolute
   cadence at all. ``exchange()`` also owns cycle timing, so an outside
   observer cannot separate "waiting for the cycle phase" from "the wire
   round" from "the NC task's own work".

   **Decision.** Split measurement by what each layer can actually
   observe:

   * The **connector** measures the quantities only it sees — wire-round
     duration, cycle-phase wait (intra-cycle slack), working-counter
     quality, per-device freshness/staleness (:need:`REQ_0262`,
     :need:`REQ_0266`, :need:`REQ_0263`, :need:`REQ_0264`). Both
     durations are *supplied to* the connector by the bus driver as
     pre-computed values; the connector holds no clock and does no tick
     arithmetic (see the clock-source contract below).
   * The **executor** measures the NC task's period jitter and deadline
     lateness (:need:`REQ_0101`, :need:`REQ_0106`). Since the NC task
     fires once per bus cycle (one-network-one-process), its period
     **is** the observable bus-cycle cadence.

   Neither layer double-counts; the two snapshots compose into the full
   picture.

   **Clock-source contract.** In v1 both durations are derived from a
   single **host monotonic** clock owned by the bus driver, so they share
   a clock domain and are directly additive. A future EtherCAT connector
   may source the wire-round duration from Distributed Clock (DC)
   timestamps for sub-microsecond accuracy; because the seam carries each
   duration as an opaque ``u32`` nanosecond value, that change needs no
   API change — only an added "different clock domain" caveat on the
   decomposition note below.

   **Composition (non-normative).** The cycle decomposes as
   ``execute_duration ≈ phase_wait + wire_round + NC work``, where
   ``execute_duration`` is the executor's NC-task figure (:need:`REQ_0262`
   note) and ``phase_wait``/``wire_round`` are the connector's. Because
   ``exchange()`` owns the phase wait, the slack lives *inside* the
   executor's ``execute_duration`` (it is idle-inside-``exchange()``, not
   idle-between-fires). Consequently ``phase_wait.min → 0`` is the leading
   indicator that ``execute_duration`` is about to trip deadline lateness
   (:need:`REQ_0106`). The two layers are joined by a shared
   ``cycle_index`` (:need:`REQ_0107`, :need:`REQ_0265`). This is a
   diagnostic relationship for consumers, not a runtime-checked invariant:
   the two figures are measured independently and a strict inequality
   check would false-positive on rounding and (future) clock skew.

   **Alternatives considered.**

   * *Connector self-instruments everything, including cadence/jitter.*
     Symmetric and uniform, but requires a monotonic clock inside the
     ``no_std`` connector seam — smuggling ``std`` (or a clock trait)
     into a layer that deliberately has none. Rejected.
   * *Executor brackets ``exchange()`` from outside and owns all timing.*
     One stats engine, but the executor cannot separate cycle-phase wait
     from wire round from NC work, so bus-cycle and wire-round quantities
     are simply not observable. Rejected: the most diagnostic numbers
     would be unmeasurable.

   **Consequences.**

   ✅ Each quantity is measured where it is actually observable; no
   clock is forced into ``no_std``.
   ✅ The connector telemetry reuses :need:`ADR_0062` and stays
   allocation-free.
   ❌ A consumer wanting the "full" timing picture must read two
   snapshots (executor + connector) and compose them by ``cycle_index``.
   The composition rule is documented above; acceptable given the
   layering.
   ❌ The shared ``cycle_index`` imposes a dual obligation on the
   *executor*: it must increment its scan count and emit ``on_cycle_stats``
   on a faulted scan too (:need:`REQ_0107`), or the counters desync from
   the first fault. The boundary is paid for on both sides.

.. building-block:: Connector cycle telemetry
   :id: BB_0054
   :status: open
   :implements: REQ_0262, REQ_0263, REQ_0264, REQ_0265, REQ_0266, REQ_0267
   :refines: ADR_0063

   ``ConnectorCycleStats`` — per-bus telemetry held by a cyclic
   connector, allocated at connector build time. Fields:

   * ``wire_round: CycleStatsCore`` — the shared ``taktora-stats``
     histogram + min/max deque over wire-round duration
     (:need:`REQ_0262`).
   * ``phase_wait_min: MinMaxDeque`` + ``phase_wait_mean`` — lean
     cycle-phase-wait tracking; windowed min and running mean only, no
     histogram (the diagnostic signal is the low tail, :need:`REQ_0266`).
   * ``cycle_index: u64`` — the connector's own per-call monotonic
     counter, incremented every ``exchange()`` regardless of outcome
     (:need:`REQ_0267`); equals the executor scan count (:need:`REQ_0107`).
   * ``wc_mismatch_count: AtomicU64`` — working-counter-mismatch counter
     (:need:`REQ_0263`).
   * ``not_all_fresh_count: AtomicU64`` — cycles that were not
     all-devices-fresh (:need:`REQ_0264`).
   * ``per_device_max_stale: [AtomicU32; N]`` — max consecutive-stale
     run per device (:need:`REQ_0264`).

   The connector receives the wire-round and phase-wait durations from
   the bus driver as ``Option<u32>`` nanosecond values (:need:`REQ_0267`):
   ``Some`` on a completed/degraded cycle, ``None`` on a hard fault. A
   ``None`` is folded into **no** duration aggregate (poison-safe), while
   ``cycle_index`` and the quality counters always update. Telemetry is
   always-on when the connector implements the extension trait. The
   per-cycle push observation and the pull snapshot of :need:`REQ_0265`
   read the published derived scalars with relaxed atomics. ``no_std``
   and allocation-free, reusing :need:`BB_0053`.

.. impl:: Connector telemetry — taktora-cyclic-fieldbus + cyclic connectors
   :id: IMPL_0072
   :status: open
   :implements: BB_0054
   :refines: REQ_0265

   Concrete Rust changes that realise :need:`BB_0054`.

   * In ``taktora-cyclic-fieldbus``: add a ``CycleObservation`` value
     type carrying ``cycle_index: u64``, ``wire_round_ns: Option<u32>``,
     ``phase_wait_ns: Option<u32>``, ``all_devices_fresh``, ``wc_ok`` and
     ``stale_device_count``; a connector-side telemetry extension trait
     with an ``on_connector_cycle(&CycleObservation)`` push hook (no-op
     default) and a ``cycle_stats()`` pull accessor, both ``no_std``.
     The two durations are supplied by the implementing connector — the
     seam owns no clock.
   * In each cyclic connector (``taktora-connector-ethercat`` first):
     hold a ``ConnectorCycleStats`` (:need:`BB_0054`); take the
     wire-round and phase-wait durations from the bus driver (host
     monotonic in v1) as ``Option<u32>``; fold ``Some`` durations into
     the aggregates and skip ``None``; fold ``CycleQuality`` /
     ``Validity`` into the working-counter and freshness counters;
     increment ``cycle_index`` and fire ``on_connector_cycle`` once per
     ``exchange()`` on every path including error (:need:`REQ_0267`); and
     expose the pull snapshot.
   * Depends on ``taktora-stats`` (:need:`BB_0053`).
