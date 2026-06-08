PLC runtime — verification
==========================

Test cases verifying the PLC runtime heart family (:need:`FEAT_0010`).
Coverage today: the bounded-time dispatch sub-feature
(:need:`FEAT_0017`) and its zero-allocation requirement
(:need:`REQ_0060`); the scan-cycle observability sub-feature
(:need:`FEAT_0021`); and the PREEMPT_RT validation harness sub-feature
(:need:`FEAT_0022`).

The test cases are grouped by area (see the toctree), mirroring the
requirement feats: bounded-time dispatch (zero-allocation plus the
pre-allocated error slot), scan-cycle observability, the PREEMPT_RT
validation harness, cyclic scan execution, event-driven I/O dispatch,
deterministic logic sequencing, the cycle-time watchdog, real-time
scheduling, cooperative shutdown, the cycle-overrun fault primitive, and
the framework internal-fault model.

.. toctree::
   :maxdepth: 2

   bounded-dispatch
   observability
   preempt-rt
   cyclic-scan
   event-io
   logic-sequencing
   watchdog
   rt-scheduling
   shutdown
   overrun-fault
   internal-fault

.. needtable::
   :types: test
   :columns: id, title, status, verifies
   :show_filters:
