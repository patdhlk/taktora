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
