Connector cycle telemetry
=========================

First-class, connector-layer timing and quality statistics for cyclic
connectors. This cluster ``:satisfies:`` :need:`FEAT_0030`.

.. feat:: Connector cycle telemetry
   :id: FEAT_0038
   :status: open
   :satisfies: FEAT_0030

   First-class, connector-layer timing and quality statistics for cyclic
   connectors — the intra-bus quantities only the connector can observe.
   Per the hybrid measurement split (:need:`ADR_0063`), the connector
   measures what it alone sees (wire-round duration, cycle-phase wait,
   working-counter quality, per-device freshness); the executor separately
   measures the cadence of the task that drives the exchange. The two
   layers share a ``cycle_index`` so a consumer can compose them
   (:need:`ADR_0063`, :need:`REQ_0107`). The telemetry reuses the shared
   ``taktora-stats`` primitive (:need:`ADR_0062`) so it stays ``no_std``
   and allocation-free, matching the ``taktora-cyclic-fieldbus`` seam.

.. req:: Wire-round duration statistics
   :id: REQ_0262
   :status: draft
   :satisfies: FEAT_0038

   A cyclic connector shall report, per bus, the duration of the wire
   round performed inside ``CyclicFieldbus::exchange()`` — the wire round
   only, excluding the cycle-phase wait that precedes it (:need:`REQ_0266`)
   — as p50 / p95 / p99 percentiles (via the :need:`ADR_0062` histogram)
   plus the exact windowed min and max. The window size is configured at
   connector build time. The per-cycle update shall be allocation-free.

   The connector holds no clock; the wire-round duration is supplied to
   it by the bus driver as a pre-computed value (:need:`ADR_0063`). A
   cycle that produced no valid wire round (a hard fault, see
   :need:`REQ_0267`) contributes no sample.

   This is distinct from the executor-measured NC-task execute duration,
   which brackets ``exchange()`` and includes the task's own work; the
   composition of the two is documented in :need:`ADR_0063`.

.. req:: Cycle-phase wait (slack) statistics
   :id: REQ_0266
   :status: draft
   :satisfies: FEAT_0038

   A cyclic connector shall report, per bus, the duration spent waiting
   for the cycle phase inside ``CyclicFieldbus::exchange()`` before the
   wire round begins — the intra-cycle slack. Because the diagnostic
   signal is the *low* tail (a shrinking wait is the leading indicator of
   an impending overrun, see :need:`ADR_0063`), the connector shall report
   the exact windowed **minimum** and a running **mean**; percentiles are
   not required. The window is the same build-time window as
   :need:`REQ_0262`. Like the wire-round duration, the wait is supplied
   by the bus driver as a pre-computed value (the connector holds no
   clock); a faulted cycle (:need:`REQ_0267`) contributes no sample. The
   per-cycle update shall be allocation-free.

.. req:: Working-counter quality counter
   :id: REQ_0263
   :status: draft
   :satisfies: FEAT_0038

   A cyclic connector shall expose a monotonic per-bus counter that
   increments on each cycle whose working-counter (or protocol-equivalent
   participation check) does not match the expected device set — i.e. the
   condition that drives a transition to :need:`REQ_0230`'s ``Degraded``
   state. The counter tracks lifetime occurrences and does not reset on
   recovery.

.. req:: Freshness and staleness statistics
   :id: REQ_0264
   :status: draft
   :satisfies: FEAT_0038

   A cyclic connector shall report, per bus, a monotonic count of cycles
   that were not all-devices-fresh (``CycleQuality::all_devices_fresh ==
   false``), and, per device, the maximum consecutive-stale run observed
   (the largest ``Validity::Stale { cycles }`` reached). These quantify
   how often and how badly devices dropped out of the cyclic exchange.

.. req:: Connector statistics query API
   :id: REQ_0265
   :status: draft
   :satisfies: FEAT_0038

   Connector cycle statistics shall be available by the same two paths as
   the executor (:need:`REQ_0103`), exposed on the connector's own
   telemetry hook (not folded into the executor's observation):

   * **Push** — a per-cycle observation
     (``cycle_index``, ``wire_round_ns``, ``phase_wait_ns``,
     ``all_devices_fresh``, ``wc_ok``, ``stale_device_count``) delivered
     once per ``exchange()`` call. The two duration fields are
     ``Option<u32>`` — ``None`` on a cycle that produced no valid value
     (:need:`REQ_0267`). The ``cycle_index`` equals the executor's
     scan count for the same cycle (:need:`REQ_0107`), the join key by
     which a consumer composes the two layers.
   * **Pull** — a borrowed snapshot of the current per-bus aggregates
     (wire-round p50/p95/p99/min/max, phase-wait min/mean,
     working-counter-mismatch count, not-all-fresh count, per-device
     max-stale), readable concurrently with the cyclic exchange via
     relaxed-atomic reads. The snapshot is per-field tear-free (each
     derived scalar is published to a relaxed atomic after the
     single-writer ``&mut`` update); it is not required to be coherent
     across fields.

   Both paths shall be allocation-free on the connector side and shall
   not require ``std`` (the cyclic-fieldbus seam is ``#![no_std]``).

.. req:: Connector push fault semantics
   :id: REQ_0267
   :status: draft
   :satisfies: FEAT_0038

   The connector shall emit a push observation (:need:`REQ_0265`) once
   per ``exchange()`` call on **every** path, including when the wire
   round returns an error, and shall increment its ``cycle_index`` on
   every call regardless of outcome — so the shared-counter invariant
   with the executor (:need:`REQ_0107`) survives a faulted cycle and the
   two push streams never desync.

   Three cycle outcomes shall be distinguishable from the observation:

   * **Completed** — ``wire_round_ns`` is ``Some`` and ``all_devices_fresh
     == true``.
   * **Degraded** — ``wire_round_ns`` is ``Some`` but ``all_devices_fresh
     == false`` and/or ``wc_ok == false`` (a working-counter mismatch or
     stale device; drives :need:`REQ_0230`'s ``Degraded`` state).
   * **Fault** — the wire round errored: ``wire_round_ns`` is ``None``.

   A ``None`` duration shall contribute **no** sample to the wire-round
   histogram or the phase-wait min/mean (:need:`REQ_0262`,
   :need:`REQ_0266`) — a faulted cycle updates only ``cycle_index`` and
   the fault/quality counters, never the duration aggregates, so the
   percentiles are not poisoned by a non-measurement.
