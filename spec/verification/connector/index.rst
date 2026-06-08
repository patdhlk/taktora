Connector framework — verification
==================================

Test cases verifying the connector framework requirements. Each ``test``
directive ``:verifies:`` one or more requirements from
:doc:`../../requirements/connector/index` (or building blocks from
:doc:`../../architecture/connector/index`). The four-layer test pyramid from the
architecture's quality strategy is reflected by the section grouping
below: unit, codec, transport integration, MQTT integration, workspace
end-to-end, and loom concurrency.

Implementation tests (Rust ``#[test]``) and the verification artefacts
on this page trace 1:1 — once the implementation lands, each ``test``
body cites the Rust test path that runs it.

.. toctree::
   :maxdepth: 2

   codec
   transport
   mqtt
   ethercat
   zenoh
   can
   cross-cutting
