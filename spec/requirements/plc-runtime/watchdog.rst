Cycle-time watchdog
===================

Foundation capability (taktora-executor v0.1): visibility into
deadline-missed events at the dispatch layer.

.. feat:: Cycle-time watchdog
   :id: FEAT_0014
   :status: open
   :satisfies: FEAT_0010

   Visibility into deadline-missed events at the dispatch layer.

.. req:: Subscriber deadline detection
   :id: REQ_0030
   :status: implemented
   :satisfies: FEAT_0014
   :links: BB_0028, TEST_0118

   The runtime shall provide a ``TriggerDeclarer::deadline(subscriber,
   deadline)`` declaration that fires the item if no event arrives at the
   subscriber within ``deadline``.

.. req:: Per-execute timing visibility
   :id: REQ_0031
   :status: implemented
   :satisfies: FEAT_0014
   :links: BB_0028, TEST_0119

   The runtime shall report each item's actual execute duration through
   ``ExecutionMonitor::post_execute(task, started_at, took, ok)``.
