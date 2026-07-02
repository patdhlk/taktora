MQTT reference connector
========================

The first concrete connector instantiating the framework's contracts. This
cluster ``:satisfies:`` :need:`FEAT_0030`.

.. feat:: MQTT reference connector
   :id: FEAT_0036
   :status: open
   :satisfies: FEAT_0030

   **Motivation.** The first concrete connector instantiating the
   framework's contracts — a ``rumqttc``-backed MQTT 3.1.1 plugin and
   gateway that exercises every seam (codec, routing, health, reconnect,
   bounded bridges) so the framework core is proven against a real
   external protocol.

   **Scope.** Bidirectional pub/sub over MQTT 3.1.1; QoS 0+1; retained
   publish; wildcard subscriptions with gateway-local fan-out; optional
   TLS; username/password auth. Reconnection is delegated to
   ``rumqttc``'s ``EventLoop`` and surfaced as ``ConnectorHealth``
   (:need:`ADR_0128`), including a terminal-``Down`` policy. Connections
   use a clean session with SUBSCRIBE replay on reconnect
   (:need:`ADR_0130`). Inbound demultiplexing runs a gateway-local topic
   matcher over deduplicated, reference-counted broker subscriptions
   (:need:`ADR_0129`). ``JsonCodec`` is the default codec
   (:need:`ADR_0131`).

   **Non-goals (deferred to follow-on specs).** QoS 2; MQTT 5.0 features
   (user properties, shared subscriptions, response topic);
   Last-Will-and-Testament; persistent (``clean_session=false``) sessions;
   client-certificate authentication. ``MsgPackCodec`` is a prerequisite
   change to ``taktora-connector-codec`` (:need:`ADR_0131`) that must land
   and publish independently before this connector cites it — it is not
   part of this connector's landing.

.. req:: MqttConnector implements Connector
   :id: REQ_0250
   :github: 168
   :status: open
   :satisfies: FEAT_0036

   The connector crate shall expose ``MqttConnector<C: PayloadCodec>``
   that implements the ``Connector`` trait with
   ``type Routing = MqttRouting``.

.. req:: MqttRouting carries topic, qos, retained
   :id: REQ_0251
   :github: 167
   :status: open
   :satisfies: FEAT_0036

   The ``MqttRouting`` struct shall carry the MQTT topic name, the QoS
   level, and a retained-message flag. It shall implement the ``Routing``
   marker trait.

.. req:: QoS 0 and 1 supported
   :id: REQ_0252
   :github: 168
   :status: open
   :satisfies: FEAT_0036

   The connector shall support MQTT QoS levels ``AtMostOnce`` (0) and
   ``AtLeastOnce`` (1). QoS 2 is deferred to a follow-on spec.

.. req:: Retained-message publish supported
   :id: REQ_0253
   :github: 168
   :status: open
   :satisfies: FEAT_0036

   When ``MqttRouting::retained`` is true, the connector shall publish the
   envelope payload as a retained MQTT message.

.. req:: Wildcard subscriptions supported
   :id: REQ_0254
   :github: 169
   :status: open
   :satisfies: FEAT_0036

   The connector shall accept inbound subscriptions whose topic includes
   the MQTT wildcards ``+`` (single-level) and ``#`` (multi-level), and
   shall demultiplex received messages to the matching ``ChannelReader``
   instance(s).

.. req:: Username/password authentication
   :id: REQ_0255
   :github: 170
   :status: open
   :satisfies: FEAT_0036

   The connector shall accept username and password credentials in
   ``MqttConnectorOptions`` and present them on the MQTT CONNECT packet.

.. req:: TLS is optional via cargo feature
   :id: REQ_0256
   :github: 170
   :status: open
   :satisfies: FEAT_0036

   The connector shall provide TLS support via ``rustls`` behind a
   default-off ``tls`` cargo feature. Client-certificate authentication
   is deferred to a follow-on spec.

.. req:: MQTT 3.1.1 baseline
   :id: REQ_0257
   :github: 170
   :status: open
   :satisfies: FEAT_0036

   The connector shall target MQTT protocol version 3.1.1. MQTT 5.0
   features (user properties, shared subscriptions, response topic) are
   deferred to a follow-on spec.

.. req:: Tokio sidecar inside the gateway crate
   :id: REQ_0258
   :github: 168
   :status: open
   :satisfies: FEAT_0036

   The MQTT gateway shall host ``rumqttc::EventLoop`` on a tokio runtime
   contained inside ``taktora-connector-mqtt``. Tokio shall not leak into
   taktora-executor's WaitSet thread.

.. req:: Bridge channels are bounded
   :id: REQ_0259
   :github: 167
   :status: open
   :satisfies: FEAT_0036

   The outbound (taktora-executor → tokio) and inbound (tokio →
   taktora-executor) bridges shall be bounded channels with configurable
   capacity in ``MqttConnectorOptions``.

.. req:: Outbound bridge saturation surfaces as BackPressure
   :id: REQ_0260
   :github: 168
   :status: open
   :satisfies: FEAT_0036

   When the outbound bridge channel is full, ``ChannelWriter::send`` shall
   return ``ConnectorError::BackPressure`` and the connector shall report
   ``ConnectorHealth::Degraded``.

.. req:: Inbound bridge saturation drops frames and signals Degraded
   :id: REQ_0261
   :github: 169
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

Reconnection and health
-----------------------

The connector delegates reconnection to ``rumqttc`` and observes liveness
as health rather than repairing it in the connector (:need:`ADR_0128`).

.. req:: Connection state maps to ConnectorHealth
   :id: REQ_0980
   :github: 169
   :status: open
   :satisfies: FEAT_0036

   The gateway shall map the MQTT connection state onto
   ``ConnectorHealth``: a pending or backing-off connection attempt shall
   report ``Connecting`` and a successful ``CONNACK`` shall report ``Up``.

.. req:: Reconnect backoff is configurable
   :id: REQ_0981
   :github: 169
   :status: open
   :satisfies: FEAT_0036

   The connector shall expose ``rumqttc``'s reconnect backoff parameters
   through ``MqttConnectorOptions`` rather than a bespoke reconnect loop.

.. req:: Auth-rejected CONNACK transitions to Down
   :id: REQ_0982
   :github: 169
   :status: open
   :satisfies: FEAT_0036

   When the broker returns a ``CONNACK`` with an authentication or
   authorization failure return code, the connector shall transition to
   ``ConnectorHealth::Down`` without further reconnect attempts.

.. req:: Reconnect-attempt ceiling transitions to Down
   :id: REQ_0983
   :github: 169
   :status: open
   :satisfies: FEAT_0036

   The connector shall transition to ``ConnectorHealth::Down`` when the
   number of consecutive failed reconnect attempts exceeds a configurable
   ceiling in ``MqttConnectorOptions``.

Session model
-------------

The connector uses a clean session and replays its subscriptions on every
reconnect (:need:`ADR_0130`).

.. req:: Clean session on CONNECT
   :id: REQ_0984
   :github: 169
   :status: open
   :satisfies: FEAT_0036

   The connector shall connect with the MQTT clean-session flag set to
   true. The flag shall be configurable via ``MqttConnectorOptions``.

.. req:: SUBSCRIBE replay on reconnect
   :id: REQ_0985
   :github: 169
   :status: open
   :satisfies: FEAT_0036

   On each reconnect ``CONNACK`` the gateway shall replay every active
   subscription from its subscription table, since the clean session
   retains no broker-side subscription state.

Wildcard demux mechanism
------------------------

These requirements refine :need:`REQ_0254` with the demultiplexing
mechanism (:need:`ADR_0129`).

.. req:: Broker subscriptions are deduplicated and reference-counted
   :id: REQ_0986
   :github: 169
   :status: open
   :satisfies: FEAT_0036

   The gateway shall register each distinct topic filter with the broker
   at most once and reference-count the channels using it, sending
   ``UNSUBSCRIBE`` only when the last channel referencing a filter is
   dropped.

.. req:: Inbound PUBLISH is matched locally and fanned out
   :id: REQ_0987
   :github: 169
   :status: open
   :satisfies: FEAT_0036

   The gateway shall match each inbound ``PUBLISH`` topic locally against
   all registered channel filters and deliver the message to every
   matching ``ChannelReader`` instance.

Codec default
-------------

.. req:: JsonCodec is the default codec
   :id: REQ_0988
   :github: 167
   :status: open
   :satisfies: FEAT_0036

   The MQTT connector's examples and integration tests shall use
   ``JsonCodec`` as the default codec (:need:`ADR_0131`).
