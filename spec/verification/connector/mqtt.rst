MQTT reference connector tests
==============================

Layer-1 (pure-logic) tests run in default CI against ``MockMqttSession``
and require no real broker; they live as ``#[cfg(test)]`` units in
``crates/taktora-connector-mqtt/src`` and as integration tests in
``crates/taktora-connector-mqtt-tests/tests``. Layer-3 broker-in-CI tests
sit behind the ``rumqttc-integration`` cargo feature, run against a
``mosquitto`` fixture on ``.github/workflows/ci-mqtt.yml``, and **skip**
(early-return) when the broker environment is absent, so they compile and
noop locally.

.. test:: MqttConnector implements Connector
   :id: TEST_0957
   :status: implemented
   :verifies: REQ_0211, REQ_0220, REQ_0250

   Compile-time API-surface check plus a runtime smoke over
   ``MockMqttSession``: ``MqttConnector<JsonCodec>`` satisfies
   ``Connector`` with ``Routing = MqttRouting``, ``name()`` is ``"mqtt"``,
   a fresh connector reports ``Connecting``, and ``create_writer`` /
   ``create_reader`` return concrete ``ChannelWriter<T, JsonCodec, N>`` /
   ``ChannelReader<T, JsonCodec, N>`` handles rather than boxed trait
   objects. Realised as
   ``crates/taktora-connector-mqtt-tests/tests/connector_trait.rs``.

.. test:: MqttRouting carries topic, qos, retained
   :id: TEST_0958
   :status: implemented
   :verifies: REQ_0222, REQ_0224, REQ_0251

   Unit tests asserting ``MqttRouting`` satisfies the ``Routing`` marker
   bounds (``Clone + Send + Sync + Debug + 'static``), exposes ``topic()``
   / ``qos()`` / ``retained()`` (retained defaults false, flipped by
   ``with_retained``), derives its subscription filter from the concrete
   topic unless an explicit ``with_filter`` overrides it, and that
   ``MqttQos`` maps to wire values 0 and 1. Realised as the
   ``#[cfg(test)]`` module in
   ``crates/taktora-connector-mqtt/src/routing.rs``.

.. test:: Topic and filter grammar validation
   :id: TEST_0959
   :status: implemented
   :verifies: REQ_0254

   Table-driven and ``proptest``-driven unit tests over the MQTT 3.1.1
   name grammar: publish topics reject wildcards, NUL and space and the
   empty string; subscription filters accept ``+`` (single-level), ``#``
   (multi-level) and a leading slash, and reject ``+``/``#`` that are not
   alone in their level or a ``#`` that is not the final level. This is
   the grammar that lets the connector accept wildcard subscriptions.
   Realised as the ``#[cfg(test)]`` module in
   ``crates/taktora-connector-mqtt/src/topic.rs``.

.. test:: Wildcard topic-matcher semantics
   :id: TEST_0960
   :status: implemented
   :verifies: REQ_0254, REQ_0987

   A table of ``topic_matches(filter, topic)`` cases drawn from the MQTT
   specification (``+`` matches exactly one level, ``#`` matches the
   parent level and any extension, leading-slash edge cases) plus
   ``proptest`` invariants: ``#`` matches every concrete topic, an
   exact filter matches only its own topic, ``prefix/#`` matches ``prefix``
   and every extension, and ``prefix/+`` matches exactly one extra level.
   This is the local matcher the gateway runs over registered filters.
   Realised as the ``#[cfg(test)]`` module in
   ``crates/taktora-connector-mqtt/src/matcher.rs``.

.. test:: Outbound publish preserves QoS and retained
   :id: TEST_0961
   :status: implemented
   :verifies: REQ_0252, REQ_0253

   A typed value written through ``ChannelWriter::send`` is drained by
   ``dispatch_outbound_once`` and reaches ``MockMqttSession::publish`` with
   the correct topic, QoS and retained flag: ``AtLeastOnce`` + retained,
   and ``AtMostOnce`` + not-retained, with the JSON payload decoding back
   to the sent value. The unit test ``qos_to_rumqttc`` additionally pins
   the mapping ``AtMostOnce → QoS::AtMostOnce``, ``AtLeastOnce →
   QoS::AtLeastOnce``. Realised as
   ``crates/taktora-connector-mqtt-tests/tests/outbound.rs`` and the
   ``#[cfg(test)]`` module in
   ``crates/taktora-connector-mqtt/src/real.rs``.

.. test:: Tokio sidecar contained in the gateway crate
   :id: TEST_0962
   :status: implemented
   :verifies: REQ_0258

   ``register_with`` spawns the outbound-drain dispatcher on the
   crate-contained tokio runtime, and a subsequent ``ChannelWriter::send``
   reaches the session asynchronously without the test naming any tokio
   type; health flips to ``Up`` after registration. The static half is a
   ``cargo public-api`` surface scan asserting no ``tokio::`` type appears
   in the crate's public API (CI-gated; ``#[ignore]`` locally). Realised as
   ``register_with_drives_outbound_publish_to_session`` in
   ``crates/taktora-connector-mqtt-tests/tests/outbound.rs`` and
   ``crates/taktora-connector-mqtt/tests/tokio_containment.rs``. The
   runtime piece — asserting no MQTT sidecar task handle appears in the
   executor's WaitSet thread — is deferred to a future stage that lands
   ``taktora-executor`` task introspection (mirrors :need:`TEST_0314`).

.. test:: Bridge channels are bounded and capacity-clamped
   :id: TEST_0963
   :status: implemented
   :verifies: REQ_0259

   Unit tests asserting the outbound and inbound bridges are bounded
   channels: ``OutboundBridge`` honours its configured capacity and clamps
   a zero capacity to 1, and ``MqttConnectorOptions`` round-trips the
   configurable ``outbound_bridge_capacity`` / ``inbound_bridge_capacity``
   through its builder while clamping a zero request to a usable size of 1.
   Realised as the ``#[cfg(test)]`` modules in
   ``crates/taktora-connector-mqtt/src/bridge.rs`` and
   ``crates/taktora-connector-mqtt/src/options.rs``.

.. test:: Outbound-bridge saturation surfaces as BackPressure
   :id: TEST_0964
   :status: implemented
   :verifies: REQ_0260

   With a full ``BridgedOutbound`` (capacity 1), ``try_send`` returns
   ``ConnectorError::BackPressure`` and folds a single ``Up → Degraded``
   transition whose reason mentions ``backpressure`` into the shared
   ``MqttHealthMonitor``; the transition is latched (repeat saturation
   emits nothing) until a recovery to ``Up`` re-arms it, and draining a
   slot lets the gate accept again. Realised as
   ``crates/taktora-connector-mqtt/tests/saturation.rs`` and the health
   ``record_outbound_backpressure`` unit path in
   ``crates/taktora-connector-mqtt/src/health.rs``.

.. test:: Inbound-bridge saturation drops frames and signals Degraded
   :id: TEST_0965
   :status: implemented
   :verifies: REQ_0261

   Past capacity, the per-channel ``InboundBridge`` returns
   ``InboundOutcome::Dropped { count }`` and increments the cumulative
   drop counter; crossing the configured ``inbound_drop_threshold`` emits
   exactly one ``Up → Degraded { reason: "dropped N inbound frames" }``
   transition, latched until the connector recovers to ``Up``. Realised as
   ``crates/taktora-connector-mqtt-tests/tests/saturation.rs``, the
   ``InboundBridge`` unit tests in
   ``crates/taktora-connector-mqtt/src/bridge.rs``, and the
   ``record_inbound_drop`` unit path in
   ``crates/taktora-connector-mqtt/src/health.rs``.

.. test:: Inbound PUBLISH matched locally and fanned out
   :id: TEST_0966
   :status: implemented
   :verifies: REQ_0254, REQ_0987

   A simulated broker ``PUBLISH`` on ``robot/arm/telemetry`` is matched by
   the gateway against every registered channel filter and delivered to
   each matching ``ChannelReader`` — both the ``robot/+/telemetry``
   (single-level) and ``robot/arm/#`` (multi-level) readers receive it,
   while a non-matching ``robot/leg/telemetry`` reader receives nothing.
   Realised as ``inbound_publish_fans_out_to_every_matching_reader`` in
   ``crates/taktora-connector-mqtt-tests/tests/inbound.rs``.

.. test:: Broker subscriptions deduplicated and reference-counted
   :id: TEST_0967
   :status: implemented
   :verifies: REQ_0986

   Two channels sharing a filter cause exactly one broker ``SUBSCRIBE``
   (dedup) yet an inbound ``PUBLISH`` still fans out to both readers; the
   ``InboundTable`` unit test reference-counts a shared filter and fires
   the ``UNSUBSCRIBE`` (subscription-handle drop) only when the last
   channel referencing it is removed, while distinct filters each require
   their own ``SUBSCRIBE``. Realised as
   ``shared_filter_subscribes_broker_once_and_fans_out_to_all`` in
   ``crates/taktora-connector-mqtt-tests/tests/inbound.rs`` and the
   ``#[cfg(test)]`` module in
   ``crates/taktora-connector-mqtt/src/inbound.rs``.

.. test:: Connection state maps to ConnectorHealth
   :id: TEST_0968
   :status: implemented
   :verifies: REQ_0980

   The health watcher maps the session connection state onto
   ``ConnectorHealth``: fresh is ``Connecting``, a ``CONNACK`` is ``Up``, a
   transient disconnect returns to ``Connecting``, and a reconnect
   ``CONNACK`` returns to ``Up``. The ``Shared`` unit test confirms a
   successful ``CONNACK`` resets the reconnect counter while a connection
   error bumps it and stays ``Connecting``. Realised as
   ``connection_state_maps_to_health`` in
   ``crates/taktora-connector-mqtt-tests/tests/reconnect.rs`` and the
   ``shared_state_transitions_track_connack_and_errors`` unit test in
   ``crates/taktora-connector-mqtt/src/real.rs``.

.. test:: Reconnect and backoff parameters are configurable
   :id: TEST_0969
   :status: implemented
   :verifies: REQ_0981

   ``MqttConnectorOptions`` exposes the reconnect configuration through its
   builder rather than a bespoke loop: the ``reconnect_attempt_ceiling`` is
   asserted end-to-end (see :need:`TEST_0971`), and the
   ``reconnect_initial_backoff`` / ``reconnect_max_backoff`` setters carry
   the initial and maximum delay that the crate-internal reconnect loop
   consumes. Realised as the ``defaults`` / ``overrides_round_trip`` unit
   tests in ``crates/taktora-connector-mqtt/src/options.rs`` and the
   backoff wiring in ``crates/taktora-connector-mqtt/src/real.rs``. The
   backoff *timing* itself is not independently asserted — only the
   configuration surface and its consumption by the reconnect loop are
   under test.

.. test:: Auth-rejected CONNACK transitions to Down
   :id: TEST_0970
   :status: implemented
   :verifies: REQ_0982

   An authentication-rejected ``CONNACK`` drives the connector to a
   terminal ``ConnectorHealth::Down`` with no further reconnect attempts;
   the ``Shared`` unit test confirms ``on_connack(NotAuthorized)`` yields
   the ``AuthRejected`` connection state. Realised as
   ``auth_rejected_connack_transitions_to_down`` in
   ``crates/taktora-connector-mqtt-tests/tests/reconnect.rs`` and
   ``shared_state_transitions_track_connack_and_errors`` in
   ``crates/taktora-connector-mqtt/src/real.rs``.

.. test:: Reconnect-attempt ceiling transitions to Down
   :id: TEST_0971
   :status: implemented
   :verifies: REQ_0983

   With ``reconnect_attempt_ceiling(2)``, three consecutive failed
   reconnects (exceeding the ceiling) drive the connector to
   ``ConnectorHealth::Down``; ``reconnect_attempts()`` reads back 3.
   Realised as ``reconnect_ceiling_exceeded_transitions_to_down`` in
   ``crates/taktora-connector-mqtt-tests/tests/reconnect.rs``.

.. test:: Clean session on CONNECT is set and configurable
   :id: TEST_0972
   :status: implemented
   :verifies: REQ_0984

   ``MqttConnectorOptions`` defaults the clean-session flag to ``true`` and
   round-trips a builder override to ``false``; ``build_mqtt_options``
   propagates the flag onto the ``rumqttc`` ``MqttOptions``
   (``set_clean_session``), alongside keep-alive and credentials. Realised
   as the ``defaults`` / ``overrides_round_trip`` unit tests in
   ``crates/taktora-connector-mqtt/src/options.rs`` and
   ``build_mqtt_options_maps_credentials_keepalive_clean_session`` in
   ``crates/taktora-connector-mqtt/src/real.rs``.

.. test:: SUBSCRIBE replay on reconnect
   :id: TEST_0973
   :status: implemented
   :verifies: REQ_0985

   A reader subscribes its filter at ``create_reader``; after a disconnect
   drives the connector through ``Connecting`` and a reconnect ``CONNACK``
   returns it to ``Up``, the gateway replays the active subscription (the
   broker observes the same ``SUBSCRIBE`` twice), and inbound delivery
   resumes over the replayed filter. Realised as
   ``reconnect_replays_active_subscriptions`` in
   ``crates/taktora-connector-mqtt-tests/tests/reconnect.rs``.

.. test:: JsonCodec is the default codec
   :id: TEST_0974
   :status: implemented
   :verifies: REQ_0988

   A JSON-encoded struct published on a concrete topic through
   ``MockMqttSession`` is delivered to a matching wildcard subscription and
   decodes back equal to the original, and a non-matching subscription does
   not fire — exercising ``JsonCodec`` as the connector's default codec.
   Realised as
   ``crates/taktora-connector-mqtt-tests/tests/end_to_end.rs``.

.. test:: Username/password CONNECT against a real broker
   :id: TEST_0255
   :status: implemented
   :verifies: REQ_0255

   Broker-in-CI test (``rumqttc-integration``): correct credentials reach
   ``Connected`` on the CONNECT, while a wrong password drives the terminal
   ``AuthRejected`` state. The unit layer additionally round-trips the
   ``credentials`` through ``MqttConnectorOptions`` and maps them onto the
   ``rumqttc`` options. Realised as ``test_0255_auth_accept_and_reject`` in
   ``crates/taktora-connector-mqtt-tests/tests/real_broker.rs`` (skips when
   ``MQTT_TEST_BROKER`` is unset) plus the options / ``real.rs`` unit
   tests.

.. test:: TLS handshake and round-trip
   :id: TEST_0256
   :status: implemented
   :verifies: REQ_0256

   Broker-in-CI test (``rumqttc-integration`` + ``tls`` feature, gated
   additionally on ``MQTT_TEST_CA``): with a CA-pinned ``rustls``
   configuration the handshake on 8883 reaches ``Connected`` and a payload
   round-trips over the encrypted link. The unit layer confirms the CA PEM
   round-trips through ``MqttConnectorOptions``, that requesting TLS
   without the feature is a clear error, and that with the feature the
   ``rumqttc`` transport becomes ``Tls``. Realised as
   ``test_0256_tls_handshake_round_trip`` in
   ``crates/taktora-connector-mqtt-tests/tests/real_broker.rs`` plus the
   options / ``real.rs`` unit tests.

.. test:: MQTT 3.1.1 plain JSON round-trip
   :id: TEST_0257
   :status: implemented
   :verifies: REQ_0257

   Broker-in-CI test (``rumqttc-integration``): a plain-TCP CONNECT on 1883
   reaches ``Connected`` and a JSON payload publishes → broker → subscribes
   back intact through ``RealMqttSession``, exercising the ``rumqttc`` MQTT
   3.1.1 stack end-to-end. Realised as ``test_0257_plain_json_round_trip``
   in ``crates/taktora-connector-mqtt-tests/tests/real_broker.rs`` (skips
   when ``MQTT_TEST_BROKER`` is unset).
