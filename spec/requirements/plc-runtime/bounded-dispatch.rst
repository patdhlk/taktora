Bounded-time dispatch
=====================

Gap capability: the dispatch hot path shall not allocate, take unbounded
locks, or block on poll loops, so steady-state cycle latency is bounded by
factors the runtime declares (not by the system allocator or kernel futex
implementation).

.. feat:: Bounded-time dispatch
   :id: FEAT_0017
   :status: open
   :satisfies: FEAT_0010

   The dispatch hot path shall not allocate, take unbounded locks, or
   block on poll loops, so steady-state cycle latency is bounded by
   factors the runtime declares (not by the system allocator or kernel
   futex implementation).

.. req:: No heap allocation in dispatch
   :id: REQ_0060
   :status: implemented
   :satisfies: FEAT_0017
   :links: BB_0023, IMPL_0001, TEST_0170

   The runtime's dispatch path shall perform zero heap allocations during
   steady-state execution after ``Executor::run`` has been entered. All
   per-iteration data structures (error capture, vertex tracking,
   completion signalling) shall reuse capacity provisioned at
   ``Executor::build`` time.

.. req:: Statically-sized task pool
   :id: REQ_0061
   :status: open
   :satisfies: FEAT_0017

   The runtime's worker pool shall be sized at ``Executor::build`` time
   from a configuration value, and the dispatch path shall not grow or
   shrink the pool during execution.

.. req:: Wait-free completion signalling
   :id: REQ_0063
   :status: open
   :satisfies: FEAT_0017

   The graph DAG scheduler shall not rely on a polling condvar
   ``wait_timeout`` for vertex-completion signalling. Completion shall be
   communicated via a wait-free or bounded-wait primitive whose worst-case
   wakeup latency is documented and dominated by the kernel's wakeup
   delivery latency, not by an internal polling interval.

.. req:: Pre-allocated error slot
   :id: REQ_0062
   :status: implemented
   :satisfies: FEAT_0017
   :links: BB_0023, IMPL_0001, TEST_0141

   The runtime shall capture per-iteration item errors in a pre-allocated
   bounded slot rather than constructing an ``Arc<Mutex<Option<...>>>``
   per dispatch iteration.

.. req:: At-most-one borrowed-job submit per barrier phase
   :id: REQ_0854
   :status: implemented
   :satisfies: FEAT_0017
   :links: BB_0023, IMPL_0001, TEST_0872

   A *barrier phase* for a task begins when its ``pending_cycle`` token is
   set at dispatch and ends when ``barrier_and_record`` takes it. The
   dispatch path shall submit each task's borrowed job — the **main item**
   *or* the **fault handler** — **at most once per phase**: a dispatch
   requested while the token is already set shall be **skipped**, never
   re-submitted, so that two pool workers can never alias one
   ``*mut dyn FnMut``. The token is set uniformly on both the normal and the
   fault-routed dispatch branch and for every task kind, so the guard also
   covers the borrowed fault-handler submit.

   The skipped run is **sound** because the single run drains all pending
   input through its listener ``take()`` loop: the listener's notifications
   are level-readable, so a second listener fired in the same wake-phase is
   serviced by the one run rather than by a second, aliasing submit.
   Consequently each task **records at most one cycle** and advances
   ``cycle_index`` **at most once per phase**. Telemetry stays correct
   because ``record_cycle_for`` early-returns for event tasks (which carry
   no ``scan_period``), so the single recorded cycle is the cyclic one.

   This is the explicit guard that makes the at-most-one-submit contract
   hold *by construction* rather than only by accident of the per-callback
   barrier. It is the precondition for the follow-up barrier-consolidation
   slice (which removes that per-callback barrier) and it closes the latent
   Grid-path (Linux-default) hazard whereby ``run_grid_cyclic_pass`` barriers
   only after its whole due-loop, so two grid slots resolving to one task
   would otherwise double-submit the same borrowed job — see
   :need:`ADR_0105`. It complements the once-per-period cyclic contract of
   :need:`REQ_0002` and the absolute-grid phase-locking of :need:`REQ_0268`,
   and it must not introduce steady-state allocation on the dispatch path
   (:need:`REQ_0060`). The per-task lateness and cycle telemetry of
   :need:`REQ_0106` / :need:`REQ_0107` therefore see exactly one record per
   phase per task.
