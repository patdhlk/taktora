Soft-RT PLC runtime heart
=========================

This chapter captures the requirements for using ``taktora-executor`` as the runtime
heart of a soft-real-time PLC. It follows from the gap analysis between
typical PLC architecture (Beckhoff TwinCAT, Siemens TIA, B&R Automation
Studio, Rockwell Logix) and the abstractions taktora-executor provides today.

The decomposition is two-tier:

* **Top-level feature** — :need:`FEAT_0010` — the umbrella capability.
* **Sub-features** — capability themes, each one ``:satisfies:`` the
  top-level feature.
* **Requirements** — concrete shall-clauses that ``:satisfies:`` a
  sub-feature.

Sub-features are grouped into **foundation capabilities** (already provided
by taktora-executor v0.1) and **gap capabilities** (must be added before the
runtime credibly serves as a soft-RT PLC heart). Foundation reqs reference
the existing API surface; gap reqs describe TBD work.

The umbrella decomposes into fourteen sub-features, each on its own page
(see the toctree). Foundation capabilities — cyclic scan execution
(:need:`FEAT_0011`), event-driven I/O dispatch (:need:`FEAT_0012`),
deterministic logic sequencing (:need:`FEAT_0013`), cycle-time watchdog
(:need:`FEAT_0014`), real-time worker scheduling (:need:`FEAT_0015`), and
cooperative shutdown (:need:`FEAT_0016`) — already exist in
taktora-executor v0.1. Gap capabilities — bounded-time dispatch
(:need:`FEAT_0017`), cycle-overrun fault primitive (:need:`FEAT_0018`),
mode / state-machine framework (:need:`FEAT_0019`), retentive state
(:need:`FEAT_0020`), scan-cycle observability (:need:`FEAT_0021`),
PREEMPT_RT validation harness (:need:`FEAT_0022`), fieldbus integration
interface (:need:`FEAT_0023`), and the framework internal-fault model
(:need:`FEAT_0024`) — are prerequisites for credibly calling the runtime a
soft-real-time PLC heart.

Top-level feature
-----------------

.. feat:: PLC runtime heart on iceoryx2
   :id: FEAT_0010
   :status: open

   A Rust runtime that schedules, sequences, and observes the cyclic
   execution of PLC-style logic (read inputs → run logic → write outputs)
   under soft-real-time constraints, with iceoryx2 as the inter-process
   data plane.

   The runtime targets non-safety industrial automation, robotics control
   loops, and machine-monitoring scenarios. Hard-real-time bounds, safety
   certification, IEC 61131-3 frontends, hot-standby, and specific
   fieldbus protocol stacks are explicitly out of scope; the runtime
   integrates with such concerns but does not implement them.

Requirements at a glance
------------------------

.. needtable::
   :columns: id, title, status, satisfies
   :show_filters:
   :filter: "FEAT_0010" in satisfies or "FEAT_0011" in satisfies or "FEAT_0012" in satisfies or "FEAT_0013" in satisfies or "FEAT_0014" in satisfies or "FEAT_0015" in satisfies or "FEAT_0016" in satisfies or "FEAT_0017" in satisfies or "FEAT_0018" in satisfies or "FEAT_0019" in satisfies or "FEAT_0020" in satisfies or "FEAT_0021" in satisfies or "FEAT_0022" in satisfies or "FEAT_0023" in satisfies or "FEAT_0024" in satisfies

.. toctree::
   :maxdepth: 2

   cyclic-scan
   event-io
   logic-sequencing
   watchdog
   rt-scheduling
   shutdown
   bounded-dispatch
   overrun-fault
   mode-state-machine
   retentive-state
   observability
   preempt-rt
   fieldbus-integration
   internal-fault

Cross-cutting traceability
--------------------------

Every requirement in this chapter ``:satisfies:`` exactly one parent feature;
every sub-feature ``:satisfies:`` :need:`FEAT_0010`. The needtables on
this page and on :doc:`../../architecture/plc-runtime/index` will populate as
``spec`` artefacts are authored.

.. needtable::
   :types: feat
   :filter: id >= "FEAT_0010" and id <= "FEAT_0024"
   :columns: id, title, status, satisfies
   :show_filters:

Safety refinements
------------------

The PLC runtime (``taktora-executor``) carries four TSRs from the SEooC
safety concept (see :doc:`../../safety/tsc`):

* :need:`TSR_0003` (integrity-level declaration and process isolation
  for executable items) — **draft**; ``ExecutableItem`` trait and
  registration API need an integrity-level field. See :need:`ADR_0050`.
* :need:`TSR_0004` (missed-deadline detection within one cycle) —
  **implemented** by the executor's existing deadline monitor.
* :need:`TSR_0009` (cross-process hosting mode) — **draft**; the
  executor must support a mode that hosts only SC items and
  cross-references QM items via iceoryx2.
* :need:`TSR_0010` (heartbeat for Element B monitor) — **draft**; no
  liveness heartbeat surface exists today.
