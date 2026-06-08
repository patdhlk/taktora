MQTT reference connector
========================

The first concrete connector instantiating the framework's contracts. This
cluster ``:satisfies:`` :need:`FEAT_0030`.

.. feat:: MQTT reference connector
   :id: FEAT_0036
   :status: open
   :satisfies: FEAT_0030

   The first concrete connector instantiating the framework's contracts:
   ``rumqttc``-backed MQTT 3.1.1 plugin and gateway with bidirectional
   pub/sub, QoS 0+1, retained messages, wildcard subscriptions, and
   optional TLS.

.. req:: MqttConnector implements Connector
   :id: REQ_0250
   :status: open
   :satisfies: FEAT_0036

   The connector crate shall expose ``MqttConnector<C: PayloadCodec>``
   that implements the ``Connector`` trait with
   ``type Routing = MqttRouting``.

.. req:: MqttRouting carries topic, qos, retained
   :id: REQ_0251
   :status: open
   :satisfies: FEAT_0036

   The ``MqttRouting`` struct shall carry the MQTT topic name, the QoS
   level, and a retained-message flag. It shall implement the ``Routing``
   marker trait.

.. req:: QoS 0 and 1 supported
   :id: REQ_0252
   :status: open
   :satisfies: FEAT_0036

   The connector shall support MQTT QoS levels ``AtMostOnce`` (0) and
   ``AtLeastOnce`` (1). QoS 2 is deferred to a follow-on spec.

.. req:: Retained-message publish supported
   :id: REQ_0253
   :status: open
   :satisfies: FEAT_0036

   When ``MqttRouting::retained`` is true, the connector shall publish the
   envelope payload as a retained MQTT message.

.. req:: Wildcard subscriptions supported
   :id: REQ_0254
   :status: open
   :satisfies: FEAT_0036

   The connector shall accept inbound subscriptions whose topic includes
   the MQTT wildcards ``+`` (single-level) and ``#`` (multi-level), and
   shall demultiplex received messages to the matching ``ChannelReader``
   instance(s).

.. req:: Username/password authentication
   :id: REQ_0255
   :status: open
   :satisfies: FEAT_0036

   The connector shall accept username and password credentials in
   ``MqttConnectorOptions`` and present them on the MQTT CONNECT packet.

.. req:: TLS is optional via cargo feature
   :id: REQ_0256
   :status: open
   :satisfies: FEAT_0036

   The connector shall provide TLS support via ``rustls`` behind a
   default-off ``tls`` cargo feature. Client-certificate authentication
   is deferred to a follow-on spec.

.. req:: MQTT 3.1.1 baseline
   :id: REQ_0257
   :status: open
   :satisfies: FEAT_0036

   The connector shall target MQTT protocol version 3.1.1. MQTT 5.0
   features (user properties, shared subscriptions, response topic) are
   deferred to a follow-on spec.

.. req:: Tokio sidecar inside the gateway crate
   :id: REQ_0258
   :status: open
   :satisfies: FEAT_0036

   The MQTT gateway shall host ``rumqttc::EventLoop`` on a tokio runtime
   contained inside ``taktora-connector-mqtt``. Tokio shall not leak into
   taktora-executor's WaitSet thread.

.. req:: Bridge channels are bounded
   :id: REQ_0259
   :status: open
   :satisfies: FEAT_0036

   The outbound (taktora-executor → tokio) and inbound (tokio →
   taktora-executor) bridges shall be bounded channels with configurable
   capacity in ``MqttConnectorOptions``.

.. req:: Outbound bridge saturation surfaces as BackPressure
   :id: REQ_0260
   :status: open
   :satisfies: FEAT_0036

   When the outbound bridge channel is full, ``ChannelWriter::send`` shall
   return ``ConnectorError::BackPressure`` and the connector shall report
   ``ConnectorHealth::Degraded``.

.. req:: Inbound bridge saturation drops frames and signals Degraded
   :id: REQ_0261
   :status: open
   :satisfies: FEAT_0036

   When the inbound bridge channel is full, the gateway shall
   (1) increment the per-channel inbound-drop counter exposed via
   ``InboundOutcome::Dropped { count }`` on the bridge's ``try_send``
   return, (2) drop the offending message for that delivery, and
   (3) emit a ``ConnectorHealth::Degraded { reason: "dropped N inbound frames" }``
   health transition when the cumulative inbound-drop count crosses
   the connector's configured ``inbound_drop_threshold`` (default 1).
   The Degraded transition is emitted at most once until the
   connector recovers to ``Up`` via the underlying stack's recovery
   path; the cumulative drop count itself is observable through every
   subsequent ``InboundOutcome::Dropped`` return.
