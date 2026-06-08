Deterministic logic sequencing
==============================

Foundation capability (taktora-executor v0.1): items compose into chains
and DAGs with explicit ordering and abort semantics — the structural
equivalent of a PLC cause-effect network.

.. feat:: Deterministic logic sequencing
   :id: FEAT_0013
   :status: open
   :satisfies: FEAT_0010

   Items compose into chains and DAGs with explicit ordering and abort
   semantics — the structural equivalent of a PLC cause-effect network.

.. req:: Sequential chain execution
   :id: REQ_0020
   :status: implemented
   :satisfies: FEAT_0013
   :links: BB_0027, TEST_0114

   The runtime shall execute the items of a chain in declared order on a
   single dispatch slot per chain invocation.

.. req:: Parallel DAG execution
   :id: REQ_0021
   :status: implemented
   :satisfies: FEAT_0013
   :links: BB_0027, TEST_0115

   The runtime shall execute the vertices of a DAG concurrently when their
   in-edges are all satisfied, and shall block downstream vertices until
   all of their upstream vertices have completed.

.. req:: Abort propagation
   :id: REQ_0022
   :status: implemented
   :satisfies: FEAT_0013
   :links: BB_0027, TEST_0116

   An item returning ``Ok(ControlFlow::StopChain)`` or ``Err`` shall
   prevent any downstream items in its enclosing chain or DAG from being
   dispatched within the same triggering cycle.

.. req:: Conditional inclusion
   :id: REQ_0023
   :status: implemented
   :satisfies: FEAT_0013
   :links: BB_0027, TEST_0117

   The runtime shall provide a ``wrap_with_condition(item, predicate)``
   helper that gates an item's execution on a runtime-evaluated predicate.
