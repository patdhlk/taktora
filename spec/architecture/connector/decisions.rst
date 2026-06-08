Architecture decisions
======================

arc42 §9–§10.

9. Architecture decisions
-------------------------

The decisions ``ADR_0001`` through ``ADR_0010`` recorded in
:ref:`the solution strategy <connector-arc42-solution-strategy>` are the
canonical architecture decision log for this framework. This section is a
needtable view for quick browsing.

.. needtable::
   :types: arch-decision
   :columns: id, title, status, refines
   :show_filters:

----

10. Quality requirements
------------------------

The four quality goals (:need:`QG_0001`–:need:`QG_0004`) form the root
of the quality tree. Concrete quality scenarios that test them are
authored as ``test`` directives in :doc:`../../verification/connector` —
the verification artefacts are the operational form of the quality
tree. A future spec round may add an explicit quality-tree
``architecture`` element if measurement targets (latency budgets,
throughput) become first-class.
