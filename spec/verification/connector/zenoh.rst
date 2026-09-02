Zenoh reference connector
=========================

Layer-1 (pure-logic) tests run in default CI against
``MockZenohSession`` and require no real ``zenoh`` crate.
Layer-2 (``zenoh-integration`` feature gate) and Layer-3
(``ZENOH_TEST_ROUTER`` env-gated client mode) tests run on
dedicated CI jobs; they ``:status: draft`` until those jobs
land.

.. test:: ZenohRouting field validation
   :id: TEST_0300
   :status: open
   :verifies: REQ_0401

   Unit test asserting that constructing a ``ZenohRouting`` with
   an invalid ``key_expr`` (empty, leading slash, illegal wildcard
   combination) fails with ``ConnectorError::Configuration`` before
   any iceoryx2 service is created. Asserts that valid
   congestion-control / priority / reliability / express
   combinations round-trip through ``ChannelDescriptor`` without
   loss.

.. test:: ZenohConnector implements Connector with ZenohRouting
   :id: TEST_0301
   :status: open
   :verifies: REQ_0400

   Compile-fail test ensuring ``ZenohConnector<JsonCodec>`` is
   accepted in any position requiring ``Connector<Routing =
   ZenohRouting>``. Asserts ``create_writer`` / ``create_reader``
   return the expected ``ChannelWriter<T, JsonCodec, N>`` /
   ``ChannelReader<T, JsonCodec, N>`` concrete types.

.. test:: Pub/sub end-to-end against MockZenohSession
   :id: TEST_0302
   :status: implemented
   :verifies: REQ_0402, REQ_0407, REQ_0408, REQ_0445

   Drive a ``ChannelWriter::send(value)`` through
   ``MockZenohSession`` and observe ``ChannelReader::try_recv``
   receive the same value. Asserts sequence number monotonicity
   (:need:`REQ_0202`), timestamp non-zero, and that the gateway
   publishes raw bytes on the inbound service (codec runs on the
   plugin side, per :need:`REQ_0408`).

.. test:: Query round-trip against MockZenohSession
   :id: TEST_0303
   :status: implemented
   :verifies: REQ_0420, REQ_0421, REQ_0422, REQ_0423, REQ_0424, REQ_0426, REQ_0427

   End-to-end query test: plugin A calls
   ``ZenohQuerier::send(q)``; plugin B's ``ZenohQueryable::try_recv``
   surfaces ``(QueryId, Q)``; plugin B calls ``reply(id, r)`` three
   times then ``terminate(id)``; plugin A's ``ZenohQuerier::try_recv``
   observes the three replies in order followed by a 0x02
   end-of-stream envelope. Asserts ``QueryId`` round-trips through
   the envelope's ``correlation_id`` unchanged and that the
   queryable's gateway map entry is freed after ``terminate``.

.. test:: Codec failure paths for queries
   :id: TEST_0304
   :status: open
   :verifies: REQ_0427

   Encoding a value larger than the envelope's payload returns
   ``ConnectorError::Codec`` from ``ZenohQuerier::send`` and from
   ``ZenohQueryable::reply``; decoding malformed bytes returns
   ``ConnectorError::Codec`` from the matching ``try_recv``. The
   envelope is not silently dropped (re-affirming
   :need:`REQ_0214`).

.. test:: Outbound bridge saturation surfaces as BackPressure
   :id: TEST_0305
   :status: open
   :verifies: REQ_0404, REQ_0405

   With ``outbound_bridge_capacity = 1`` and a deliberately stalled
   ``MockZenohSession``, the second ``ChannelWriter::send`` (and
   the second ``ZenohQuerier::send``) returns
   ``ConnectorError::BackPressure`` and the connector's
   ``health()`` snapshot transitions to ``Degraded``.

.. test:: Inbound bridge saturation surfaces as DroppedInbound
   :id: TEST_0306
   :status: open
   :verifies: REQ_0406, REQ_0428

   With ``inbound_bridge_capacity = 1`` and a deliberately stalled
   plugin reader, the gateway emits
   ``HealthEvent::DroppedInbound { count = N }`` reflecting the
   number of pub/sub samples and reply chunks discarded. The
   in-flight ``QueryId`` is observable as a reply stream with
   fewer chunks than the upstream peer sent.

.. test:: Query timeout emits 0x03 terminator
   :id: TEST_0307
   :status: open
   :verifies: REQ_0425

   With ``query_timeout = 50 ms`` and a queryable that never
   replies, ``ZenohQuerier::try_recv`` observes a single envelope
   with ``payload[0] == 0x03`` for the in-flight ``QueryId``;
   subsequent ``try_recv`` calls for that ``QueryId`` return
   ``None``. Gateway map entry for that ``QueryId`` is freed.

.. test:: Health state machine on MockZenohSession lifecycle
   :id: TEST_0308
   :status: implemented
   :verifies: REQ_0440, REQ_0442

   Walk the mock session through ``Connecting → Up → Degraded →
   Up → Down`` and assert one ``HealthEvent`` per transition on
   the connector's health channel. Asserts each variant carries
   the documented payload (``since`` timestamp, ``reason``
   string). Realised across two test files: the
   ``Connecting → Up → Down`` legs by
   ``crates/taktora-connector-zenoh/tests/health_transitions.rs``,
   and the ``Up → Degraded → Up`` legs (driven by ``min_peers``
   threshold crossings) by
   ``crates/taktora-connector-zenoh/tests/min_peers_degraded.rs``.

.. test:: REQ_0441 anti-req — no ReconnectPolicy on session loss
   :id: TEST_0309
   :status: implemented
   :verifies: REQ_0441

   Regression-guard for the explicit anti-requirement
   :need:`REQ_0441` (which is status:rejected because the project
   deliberately excludes ``ReconnectPolicy`` from the Zenoh
   connector — see the "Anti-goals" preamble above the rejected
   ``req`` cluster in ``spec/requirements/connector.rst``). The
   ``:verifies:`` link asserts that the excluded behaviour
   remains absent; ``rejected`` parent + ``implemented`` verifier
   is the project's documented convention for anti-req checks.

   Static check that ``ZenohGateway`` exposes no
   ``ReconnectPolicy``-typed field and the
   ``ZenohConnectorOptions`` struct does not declare a
   ``reconnect_policy`` setting. Realised as
   ``crates/taktora-connector-zenoh/tests/no_reconnect_policy.rs``,
   which shells out to ``cargo public-api`` and asserts the
   public surface contains no ``ReconnectPolicy`` /
   ``reconnect_policy`` identifier.

.. test:: zenoh-integration feature gates the real zenoh dep
   :id: TEST_0310
   :status: implemented
   :verifies: REQ_0444, REQ_0445

   Build the crate twice — once with default features, once with
   ``--features zenoh-integration`` — and assert that the default
   build does not link ``zenoh`` (via ``cargo tree`` introspection)
   while the feature build does. Both builds expose
   ``MockZenohSession``. Realised as ``scripts/check_dep_gating.sh``
   invoked from the ``dep-gating`` job in
   ``.github/workflows/ci-zenoh.yml``.

.. test:: Cross-platform support
   :id: TEST_0311
   :status: implemented
   :verifies: REQ_0446

   CI matrix builds the crate on Linux, macOS, and Windows
   (default features) and runs ``cargo test`` on all three. No
   platform-specific compile errors; no platform-gated
   ``#[cfg]`` paths break. Realised as the ``build-test-default``
   matrix job in ``.github/workflows/ci-zenoh.yml``.

.. test:: Two-peer real session pub/sub
   :id: TEST_0312
   :status: draft
   :verifies: REQ_0440, REQ_0443

   Layer-2 integration test (``zenoh-integration`` feature):
   spawn two ``ZenohConnector`` instances in peer mode in the
   same process with disjoint locator listings, exchange envelopes
   through a real ``zenoh::Session`` pair, and assert payloads /
   sequence numbers round-trip correctly. Status remains
   ``deferred`` until the dedicated CI job lands.

.. test:: Client-mode router smoke
   :id: TEST_0313
   :status: draft
   :verifies: REQ_0440, REQ_0443

   Layer-3 env-gated test: when ``ZENOH_TEST_ROUTER`` names a
   reachable ``zenohd``, open a client-mode session and assert
   ``ConnectorHealth`` reaches ``Up``. Status remains
   ``deferred`` until the client-mode CI job lands.

.. test:: Tokio sidecar contained inside taktora-connector-zenoh
   :id: TEST_0314
   :status: implemented
   :verifies: REQ_0403

   Static check that the ``zenoh::Session`` and any tokio runtime
   handle live entirely inside the ``taktora-connector-zenoh`` crate.
   No public type exported by ``taktora-connector-zenoh`` shall name a
   ``tokio::*`` type in its signature (compile-time API surface scan).
   Realised as
   ``crates/taktora-connector-zenoh/tests/tokio_containment.rs``,
   which shells out to ``cargo public-api`` and asserts the public
   surface contains no ``tokio::`` identifier. The runtime piece —
   an executor unit test asserting that the WaitSet thread does not
   contain any tokio task handle attributable to the gateway
   sidecar (mirrors :need:`TEST_0211` under :need:`REQ_0321`) — is
   deferred to a Z6+ stage that lands ``taktora-executor`` task
   introspection.
