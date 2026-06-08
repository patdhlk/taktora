Cooperative shutdown
====================

Foundation capability (taktora-executor v0.1): the runtime exits cleanly
on signal or programmatic stop without leaking worker threads or
shared-memory artefacts.

.. feat:: Cooperative shutdown
   :id: FEAT_0016
   :status: open
   :satisfies: FEAT_0010

   The runtime exits cleanly on signal or programmatic stop without
   leaking worker threads or shared-memory artefacts.

.. req:: Signal-driven shutdown
   :id: REQ_0050
   :status: open
   :satisfies: FEAT_0016

   The runtime shall return cleanly from ``run()`` when SIGINT or SIGTERM
   is delivered to the process, surfacing iceoryx2's ``WaitSetRunResult``
   ``Interrupt`` and ``TerminationRequest`` variants.

.. req:: Programmatic shutdown wakeup
   :id: REQ_0051
   :status: implemented
   :satisfies: FEAT_0016
   :links: BB_0035, TEST_0129

   The runtime shall expose a clonable ``Stoppable`` handle whose
   ``stop()`` method wakes the WaitSet thread within a bounded time even
   when no other trigger is pending.
