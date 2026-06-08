Mode / state-machine framework
==============================

Gap capability: a first-class lifecycle for the runtime — distinct from
item lifecycle — that captures the operational modes typical of PLC
programs.

.. feat:: Mode / state-machine framework
   :id: FEAT_0019
   :status: open
   :satisfies: FEAT_0010

   A first-class lifecycle for the runtime — distinct from item lifecycle
   — that captures the operational modes typical of PLC programs.

.. req:: Mode lifecycle
   :id: REQ_0080
   :status: open
   :satisfies: FEAT_0019

   The runtime shall support an explicit mode lifecycle of at least
   ``{init, ready, running, fault, stopping, stopped}`` and shall expose
   the current mode through a query API.

.. req:: Mode transition triggers
   :id: REQ_0081
   :status: open
   :satisfies: FEAT_0019

   Mode transitions shall be triggered both programmatically (caller-driven)
   and as a consequence of configured events (executor-wide deadline
   overrun, item error, signal-driven stop).

.. req:: Per-mode task gating
   :id: REQ_0082
   :status: open
   :satisfies: FEAT_0019

   Each registered task shall declare which modes it is enabled in; the
   runtime shall not dispatch a task while it is disabled by the current
   mode.

.. req:: Mode change observability
   :id: REQ_0083
   :status: open
   :satisfies: FEAT_0019

   Mode transitions shall be visible to the configured ``Observer`` via
   a dedicated callback that reports the previous mode, the new mode, and
   the reason for the transition.
