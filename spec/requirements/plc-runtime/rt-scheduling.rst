Real-time scheduling
====================

Foundation capability (taktora-executor v0.1): worker threads can be
pinned and prioritized for predictable latency on PREEMPT_RT-capable Linux
systems.

.. feat:: Real-time worker scheduling
   :id: FEAT_0015
   :status: implemented
   :satisfies: FEAT_0010

   Worker threads can be pinned and prioritized for predictable latency on
   PREEMPT_RT-capable Linux systems.

.. req:: Core-affinity assignment
   :id: REQ_0040
   :status: implemented
   :satisfies: FEAT_0015
   :links: BB_0029, TEST_0127

   The runtime shall, behind the ``thread_attrs`` feature, allow worker
   threads to be pinned to a specified set of CPU cores.

.. req:: SCHED_FIFO priority on Linux
   :id: REQ_0041
   :status: implemented
   :satisfies: FEAT_0015
   :links: BB_0029, TEST_0128

   The runtime shall, behind the ``thread_attrs`` feature on Linux, allow
   worker threads to run under ``SCHED_FIFO`` at a configured priority,
   subject to the process holding ``CAP_SYS_NICE``.
