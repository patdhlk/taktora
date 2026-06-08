Cross-cutting tests
===================

Unit tests
----------

Per-crate, no IPC, parallel-safe.

.. test:: ExponentialBackoff invariants
   :id: TEST_0100
   :status: open
   :verifies: REQ_0233

   Property test (``proptest``) on ``ExponentialBackoff`` confirming:
   delays are monotonically non-decreasing until the cap is reached,
   delays never exceed the configured maximum, ``reset()`` returns the
   policy to the initial delay, and jitter stays within the configured
   ratio. Lives under ``taktora-connector-core/tests/``.

.. test:: ConnectorHealth state-machine transitions
   :id: TEST_0101
   :status: open
   :verifies: REQ_0230, REQ_0234

   Unit test asserting that every valid transition between
   ``ConnectorHealth`` variants (per :need:`ARCH_0012`) emits exactly
   one ``HealthEvent`` on the connector's health channel, and that
   illegal transitions panic in debug builds.

.. test:: MqttRouting wildcard demux predicate
   :id: TEST_0102
   :status: open
   :verifies: REQ_0254

   Unit-level coverage of the topic-match predicate independent of any
   broker or iceoryx2 service: every (subscription pattern, incoming
   topic) pair is asserted against the MQTT 3.1.1 wildcard semantics
   (single-level ``+``, multi-level ``#``).

.. test:: ChannelDescriptor validation
   :id: TEST_0103
   :status: open
   :verifies: REQ_0201, REQ_0221

   Asserts that constructing a ``ChannelDescriptor`` with an empty
   name fails, and that the const-generic ``N`` propagates correctly
   through ``create_writer`` / ``create_reader`` (compile-fail tests
   ensure mismatched ``N`` values do not type-check).

----

Workspace end-to-end tests
---------------------------

Full stack exercised via ``taktora-connector-host`` examples or
``assert_cmd``-driven binary smoke tests.

.. test:: In-process gateway smoke
   :id: TEST_0150
   :status: open
   :verifies: REQ_0241, ARCH_0020

   Single-binary integration: ``ConnectorHost`` launches the plugin
   executor and an in-process tokio task hosting ``MqttGateway``
   against a ``rumqttd`` fixture. End-to-end pub/sub round-trip
   succeeds; process exits cleanly on programmatic stop.

.. test:: Separate-process gateway smoke
   :id: TEST_0151
   :status: open
   :verifies: REQ_0242, ARCH_0021

   Two binaries: a plugin process running ``ConnectorHost`` and a
   gateway process running ``ConnectorGateway`` against
   ``rumqttd``. SHM transport carries envelopes between them. A
   round-trip succeeds; both processes exit cleanly.

.. test:: SIGINT clean exit within 5-second budget
   :id: TEST_0152
   :status: open
   :verifies: REQ_0243, ARCH_0013

   While the connector is mid-traffic, send SIGINT; the host returns
   from ``run()`` within 5 seconds; tokio runtime drains; all
   iceoryx2 services release; exit code is 0.

.. test:: No control-plane envelopes flow
   :id: TEST_0153
   :status: open
   :verifies: REQ_0244, REQ_0291

   With one channel configured, observe the iceoryx2 service for the
   duration of a normal session: the only envelopes that flow are
   user-payload envelopes (no "ping", "version", or "shutdown
   handshake"). Asserts the framework's no-control-plane invariant.

----

Loom concurrency tests
----------------------

Run with ``cargo test --features loom`` under ``cfg(loom)``.

.. test:: Bridge handoff under arbitrary interleaving
   :id: TEST_0160
   :status: open
   :verifies: REQ_0259, BB_0022

   Loom model of ``OutboundGatewayItem.execute`` racing with the
   tokio task draining the bridge: every produced frame is observed
   exactly once by the consumer; no deadlock.

.. test:: Health state-machine under concurrent updates
   :id: TEST_0161
   :status: open
   :verifies: REQ_0230, REQ_0234

   Loom model with multiple threads attempting transitions
   simultaneously (e.g. the tokio task reporting ``Down`` while the
   reconnect timer fires ``Connecting``): the state machine never
   enters an invalid state and no event is dropped.

----

Cross-cutting traceability
--------------------------

.. needtable::
   :types: test
   :filter: "TEST_01" in id or "TEST_02" in id or "TEST_03" in id or "TEST_04" in id
   :columns: id, title, status, verifies
   :show_filters:
