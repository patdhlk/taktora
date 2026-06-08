PLC runtime — architecture
==========================

Detailed-design notes for the soft-real-time PLC heart family
(:need:`FEAT_0010`). This chapter currently covers the **bounded-time
dispatch** sub-feature (:need:`FEAT_0017`) and its zero-allocation
guarantee (:need:`REQ_0060`), the **scan-cycle observability**
sub-feature (:need:`FEAT_0021`), the **PREEMPT_RT validation harness**
(:need:`FEAT_0022`) together with the cycle-overrun fault primitive
(:need:`FEAT_0018`) and framework internal-fault model
(:need:`FEAT_0024`), and the **absolute-grid cyclic dispatch** of
:need:`FEAT_0011`; other sub-features are added as their designs land.

Per the arc42 conventions used across this spec, design decisions are
captured as ``arch-decision`` directives, structural elements as
``building-block`` directives, and concrete code mappings as ``impl``
directives. Test cases live in :doc:`../../verification/plc-runtime/index`.

This chapter is split across pages (see the toctree): the **Solution
strategy** framing lives here on the index; the building-block
decomposition (the pre-allocated dispatch scratch and the foundation
executor surfaces) lives in :doc:`building-blocks`; the concrete
zero-allocation refactor lives in :doc:`implementation`; the scan-cycle
observability design lives in :doc:`observability`; the PREEMPT_RT
validation harness, the cycle-overrun fault primitive, and the framework
internal-fault model live in :doc:`preempt-rt`; and the absolute-grid
cyclic dispatch design lives in :doc:`absolute-grid-dispatch`.

.. toctree::
   :maxdepth: 2

   building-blocks
   implementation
   observability
   preempt-rt
   absolute-grid-dispatch

----

Solution strategy
-----------------

The dispatch hot path's zero-allocation goal is solved by **moving every
per-iteration allocation up to ``Executor::build`` time** and reusing
that capacity. Two design choices follow from that posture: how to
reuse the per-iteration error slot, and how to replace the unbounded
crossbeam re-dispatch channel that ``Graph::run_once`` allocates today.

The per-scan-cycle dispatch flow the rest of this chapter refines is: the
WaitSet thread blocks until a trigger fires (a grid ``timerfd`` tick, an
iceoryx2 event, or a stop notification), the dispatch loop brackets the
task-logic call with the ``ExecutionMonitor`` reference point
(``pre_execute`` = task-logic start), runs the item (or its fault handler),
and folds the post-execute timing into the task's cycle statistics before
re-entering the wait.

.. mermaid::

   flowchart TD
       Wait["WaitSet blocks on triggers<br/>(grid timerfd tick · iceoryx2 event · stop notify)"]
       Wake{"wake cause?"}
       EINTR["bare EINTR — dispatch nothing,<br/>count nothing, re-enter wait"]
       Stop["stop request / SIGINT-SIGTERM<br/>→ return Ok(())"]
       Pre["pre_execute reference point<br/>(task-logic start, telemetry clock)"]
       Faulted{"task faulted?"}
       Exec["execute item logic"]
       Handler["dispatch fault handler<br/>once per cycle"]
       Post["post_execute: fold took / jitter /<br/>lateness into CycleStats"]
       Obs["on_cycle_stats(&CycleObservation)<br/>(once per scan attempt, incl. faulted)"]

       Wait --> Wake
       Wake -->|"EINTR"| EINTR --> Wait
       Wake -->|"stop / termination"| Stop
       Wake -->|"grid tick / event"| Pre
       Pre --> Faulted
       Faulted -->|"no"| Exec --> Post
       Faulted -->|"yes"| Handler --> Post
       Post --> Obs --> Wait

.. arch-decision:: Pre-allocate dispatch scratch at Executor::build time
   :id: ADR_0011
   :status: open
   :refines: REQ_0060

   **Context.** Today ``Executor::dispatch_loop`` allocates
   ``Arc<Mutex<Option<ExecutorError>>>`` on every iteration
   (``executor.rs:557-558``) and ``Graph::run_once`` allocates a
   fresh ``Vec<AtomicUsize>`` counter table, a fresh
   ``Arc<GraphRuntime>``, and a fresh
   ``crossbeam_channel::unbounded::<usize>()`` on every dispatch
   (``graph.rs:276-302``). None of those shapes change between
   iterations — vertex count, successor map, and error-channel
   width are fixed once ``Executor::build`` returns.

   **Decision.** Provision all per-iteration scratch at
   ``Executor::build`` time and reset (rather than reallocate) it
   on each tick of the dispatch loop. Concretely: hoist the
   error-capture slot onto ``Executor``, hoist the runtime
   counters / pending counter / successor borrow onto ``Graph``,
   and replace the unbounded re-dispatch channel with a
   hand-rolled bounded SPSC ring whose capacity is
   ``next_power_of_two(n_vertices)`` (see :need:`BB_0023`).

   **Alternatives considered.**

   * *Slab/arena per iteration.* Trades unconditional allocation
     for a slab reset, but slabs still allocate on resize and
     hide cost in the slab implementation. Rejected — the shapes
     are statically known, so a typed pre-allocation is sharper.
   * *Switch to ``smallvec`` everywhere.* Inline storage avoids
     small allocations but spills to the heap on overflow, which
     is non-deterministic — incompatible with a soft-real-time
     guarantee.
   * *Keep ``crossbeam_channel`` but call ``bounded(n)`` once.*
     Bounded crossbeam channels still allocate Arc'd shared state
     at construction, which is acceptable at build time but adds
     an external dependency we do not need on the hot path. A
     hand-rolled SPSC ring is a few dozen lines and removes the
     send-side allocation question entirely.

   **Consequences.**

   ✅ Steady-state dispatch performs zero heap allocations
   (per :need:`REQ_0060`).
   ✅ Worst-case re-dispatch latency is bounded by ring capacity,
   not allocator behaviour.
   ❌ Adds one ``unsafe`` block to ``taktora-executor`` (the SPSC
   ring push/pop), justified by a ``// SAFETY:`` comment and
   covered by ``loom`` tests under feature flag.
   ❌ Vertex count is now an explicit ``Executor::build`` input —
   builders that add vertices after build must rebuild
   (already the case in practice; documented explicitly).
