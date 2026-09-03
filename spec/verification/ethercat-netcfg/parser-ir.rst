Parser and IR tests
===================

Test cases for the parser crate (:need:`FEAT_0081`). Per-crate, no I/O
beyond test fixtures, parallel-safe. Live under
``crates/ethercat-netcfg/tests/``.

.. test:: parse() accepts a representative WAGO network.yaml
   :id: TEST_0830
   :status: implemented
   :verifies: REQ_0820, REQ_0821

   Loads a canonical ``network.yaml`` fixture describing a WAGO 750-354
   coupler with two channels, calls the parse entry point, and asserts
   the resulting ``NetworkConfig`` carries the expected ``BusConfig``
   (cycle time, ``max_subdevices`` / ``max_pdi_bytes``), one
   ``DeviceInstance`` with its ``label`` and ``DeviceSource``, and two
   ``ChannelBinding`` entries with the expected direction, bit offset,
   and bit length.

.. test:: Multi-bus document is rejected
   :id: TEST_0831
   :status: implemented
   :verifies: REQ_0822

   A fixture declaring two top-level buses in one document parses to an
   error naming the one-file-one-bus rule. A single-bus fixture parses
   without error. Guards the scope boundary of :need:`ADR_0096`.

.. test:: Channels resolve to devices by label, stable under reorder
   :id: TEST_0832
   :status: implemented
   :verifies: REQ_0823

   A fixture whose channels reference devices by ``label`` parses
   correctly; reordering the device list in the fixture (without editing
   any channel) leaves every channel bound to the same device. Confirms
   bindings key on ``label``, not list index or address.

.. test:: Parser is independent of the connector runtime
   :id: TEST_0833
   :status: open
   :verifies: REQ_0824

   ``cargo tree -p ethercat-netcfg`` shall not list
   ``taktora-connector-ethercat`` anywhere in the resolved graph.
   Implemented as a CI shell check that greps the output and fails on
   match. ``ethercat-esi`` and ``fieldbus-od-core`` are expected
   present.
