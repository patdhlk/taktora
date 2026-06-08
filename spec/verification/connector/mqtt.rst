MQTT integration tests
======================

Embedded ``rumqttd`` per-test fixture on an ephemeral port; iceoryx2
services per test as before; one tokio runtime per test.

.. test:: QoS 0 round-trip
   :id: TEST_0130
   :status: open
   :verifies: REQ_0252

   Plugin → gateway → broker → gateway → plugin round-trip with
   ``MqttRouting { qos: AtMostOnce, retained: false }``. Asserts the
   payload bytes are preserved end-to-end.

.. test:: QoS 1 round-trip
   :id: TEST_0131
   :status: open
   :verifies: REQ_0252

   Same as TEST_0130 but with ``qos: AtLeastOnce``. Additionally
   asserts a ``PUBACK`` is observed on the gateway side before
   reporting success.

.. test:: Retained-message publish + subscribe
   :id: TEST_0132
   :status: open
   :verifies: REQ_0253

   Publish with ``retained: true``; a subsequent subscribe receives
   the retained payload as the first message. Publish a second
   payload with ``retained: false`` and verify the retained value is
   not overwritten by an unset retained.

.. test:: Wildcard subscription with `+`
   :id: TEST_0133
   :status: open
   :verifies: REQ_0254

   Subscribe with ``plant/+/temperature``; publish to
   ``plant/A/temperature`` and ``plant/B/temperature`` and
   ``plant/A/B/temperature``. Reader receives the first two; not the
   third.

.. test:: Wildcard subscription with `#`
   :id: TEST_0134
   :status: open
   :verifies: REQ_0254

   Subscribe with ``plant/#``; publish to ``plant/A``,
   ``plant/A/B``, ``plant/A/B/C``. Reader receives all three.

.. test:: Username/password authentication
   :id: TEST_0135
   :status: open
   :verifies: REQ_0255

   ``MqttConnectorOptions`` configured with username + password;
   ``rumqttd`` fixture configured to require credentials. CONNECT
   succeeds; a wrong-credential variant of the same test fails with
   ``ConnectorHealth::Down { reason: "auth" }``.

.. test:: TLS connection (developer-machine only)
   :id: TEST_0136
   :status: open
   :verifies: REQ_0256

   ``rumqttd`` fixture configured with a self-signed cert; the
   ``tls`` cargo feature is enabled; ``MqttConnectorOptions`` points
   at the test cert. CONNECT succeeds. **Not run in CI** — gated
   behind ``cfg(feature = "tls")`` and a ``CONNECTOR_MQTT_TLS_TESTS``
   env var so the repo carries no embedded test certs.

.. test:: Reconnect after broker bounce
   :id: TEST_0137
   :status: open
   :verifies: REQ_0232, REQ_0233

   While the connector is ``Up``, kill the ``rumqttd`` fixture;
   observe transition to ``Down`` then ``Connecting``; restart the
   broker; observe transition back to ``Up`` within
   ``ExponentialBackoff::max_delay`` seconds. Counts of HealthEvent
   transitions are asserted.

.. test:: HealthEvent emitted on every transition
   :id: TEST_0138
   :status: open
   :verifies: REQ_0234

   Drives the connector through every legal transition in
   :need:`ARCH_0012` and asserts a ``HealthEvent`` arrives on
   ``subscribe_health()`` for each one, in the order driven.

.. test:: Outbound bridge saturation → BackPressure
   :id: TEST_0139
   :status: open
   :verifies: REQ_0260

   Configure ``MqttConnectorOptions`` with a tiny outbound-bridge
   capacity (e.g. 2). Stop draining the gateway by holding the
   tokio task busy. Send N > 2 messages; the (N-1)th or Nth send
   returns ``Err(ConnectorError::BackPressure)`` and the connector
   transitions to ``ConnectorHealth::Degraded``.

.. test:: Inbound bridge saturation → DroppedInbound
   :id: TEST_0140
   :status: open
   :verifies: REQ_0261

   Configure a tiny inbound-bridge capacity. Block the inbound
   gateway item from draining (e.g. by holding ``ChannelReader``).
   Publish a flood of QoS 0 messages from the broker fixture; the
   gateway emits ``HealthEvent::DroppedInbound { count }`` with
   ``count > 0``. For QoS 1 traffic, ``PUBACK`` is observably
   delayed until the bridge drains.
