PREEMPT_RT validation harness
=============================

Gap capability: the runtime's worst-case latency on PREEMPT_RT Linux is
characterised under realistic load — a continuous regression gate, not a
one-off measurement.

.. feat:: PREEMPT_RT validation harness
   :id: FEAT_0022
   :status: open
   :satisfies: FEAT_0010

   The runtime's worst-case latency on PREEMPT_RT Linux is characterised
   under realistic load — a continuous regression gate, not a one-off
   measurement.

.. req:: Documented worst-case jitter
   :id: REQ_0110
   :status: draft
   :satisfies: FEAT_0022

   The repository shall ship a versioned document
   (``spec/safety/preempt-rt-envelope.rst`` or sibling) recording an
   observed worst-case jitter envelope on at least one PREEMPT_RT Linux
   configuration. The document shall record: kernel version (e.g.
   ``6.6.0-rt8``), isolation flags applied (``isolcpus``, ``nohz_full``,
   ``rcu_nocbs``), CPU model and core-pinning layout, load profile
   selected (see :need:`REQ_0111`), and the observed p50 / p95 / p99 /
   max jitter values per task as reported by :need:`REQ_0100` and
   :need:`REQ_0101`.

.. req:: Cyclictest-style benchmark harness
   :id: REQ_0111
   :status: draft
   :satisfies: FEAT_0022

   The repository shall include a benchmark harness, packaged as a
   cargo binary under ``xtask/preempt-rt/``, that exercises the
   ``taktora-executor`` dispatch path under a configured load profile and
   emits per-cycle latency observations as NDJSON to stdout.

   Each NDJSON record shall be emitted once per scan cycle (including a
   faulted scan, see :need:`REQ_0107`) and shall conform to the schema
   ``{ cycle_index: u64, task_id: u32, faulted: bool, ts_ns: u64,
   period_ns: u64, actual_period_ns: u64|null, jitter_ns: u64|null,
   lateness_ns: i64|null, took_ns: u64|null, skipped_slots: u32 }``. Consistent with the
   absent-vs-zero contract of :need:`REQ_0103`, a measurement not taken this
   cycle (first cycle, or any faulted scan) shall be encoded as JSON
   ``null``, never as a measured ``0``. ``ts_ns`` is the task-logic-start
   instant carried as ``pre_ns`` in :need:`REQ_0103`; ``task_id`` is the
   ``task_index`` of :need:`REQ_0103`. ``skipped_slots`` is always a
   number (never ``null``), per :need:`REQ_0840`.

   The harness shall offer at least three selectable load profiles:

   * ``idle`` — no co-resident load; baseline measurement.
   * ``cpu-stress`` — ``stress-ng --cpu N``-style background load on
     non-isolated cores.
   * ``cyclictest-coexist`` — runs alongside ``cyclictest`` so the two
     measurements can be cross-checked.

   *Implementation status.* The ``idle`` profile and the NDJSON export path
   (the off-RT-thread overwrite-oldest telemetry ring and its drain) are
   implemented in ``taktora-telemetry-export`` and the ``xtask/preempt-rt``
   harness. The ``cpu-stress`` and ``cyclictest-coexist`` profiles remain a
   tracked follow-up; this requirement stays ``draft`` until all three ship.

.. req:: Documented reproducer procedure
   :id: REQ_0112
   :status: draft
   :satisfies: FEAT_0022

   The repository shall ship a documented procedure
   (``docs/preempt-rt-procedure.md``) by which any maintainer with
   access to a PREEMPT_RT-equipped Linux host can reproduce the
   :need:`REQ_0110` envelope. The procedure shall be runnable from a
   single ``cargo xtask preempt-rt-bench`` invocation given a
   pre-installed PREEMPT_RT kernel and the documented isolation flags.

   A continuous CI gate on jitter is explicitly **not** required by
   this REQ. See :need:`ADR_0061` for the rationale (no persistent
   self-hosted RT runner; cloud GH runners cannot guarantee
   PREEMPT_RT).

.. req:: Harness consumes runtime telemetry
   :id: REQ_0113
   :status: draft
   :satisfies: FEAT_0022
   :refines: REQ_0103

   The benchmark harness (:need:`REQ_0111`) shall obtain its per-cycle
   observations exclusively via the ``Observer::on_cycle_stats`` push
   callback defined in :need:`REQ_0103`. The harness shall not
   instantiate a parallel timing path (no direct ``Instant::now``
   polling around ``execute``); what the harness measures is what the
   runtime would report in production.
