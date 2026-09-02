Zenoh pub/sub
=============

The pub/sub half of the Zenoh connector. This cluster ``:satisfies:``
:need:`FEAT_0042`.

.. feat:: Zenoh pub/sub
   :id: FEAT_0043
   :status: open
   :satisfies: FEAT_0042

   The pub/sub half of the Zenoh connector. ``ChannelWriter`` and
   ``ChannelReader`` carry codec-encoded values through iceoryx2 SHM
   services to / from Zenoh publishers and subscribers running on
   the gateway's tokio sidecar. Bridges between taktora-executor and
   tokio are bounded; saturation surfaces as ``BackPressure`` on
   outbound and ``DroppedInbound`` health events on inbound.

.. req:: ZenohConnector implements Connector
   :id: REQ_0400
   :status: implemented
   :satisfies: FEAT_0043
   :links: BB_0040, BB_0041, TEST_0301

   The connector crate shall expose ``ZenohConnector<C: PayloadCodec>``
   that implements the ``Connector`` trait with
   ``type Routing = ZenohRouting`` and ``type Codec = C``.

.. req:: ZenohRouting carries key_expr and pub/sub QoS fields
   :id: REQ_0401
   :status: implemented
   :satisfies: FEAT_0043
   :links: BB_0041, TEST_0300

   The ``ZenohRouting`` struct shall carry the Zenoh key expression
   (``key_expr: KeyExprOwned``), congestion control mode
   (``Block | Drop``), priority (``RealTime..Background``),
   reliability (``Reliable | BestEffort``), and a boolean
   ``express`` flag (batching opt-out). It shall implement the
   ``Routing`` marker trait. Validation of the key expression shall
   occur on the plugin side inside ``create_writer`` /
   ``create_reader`` (and the query-side analogues), before any
   iceoryx2 service is created; an invalid expression shall yield
   ``ConnectorError::Configuration``.

.. req:: JsonCodec is the default codec for Zenoh
   :id: REQ_0402
   :status: implemented
   :satisfies: FEAT_0043
   :links: IMPL_0060, TEST_0302

   The Zenoh connector shall accept any ``PayloadCodec`` via its
   ``C`` generic parameter (re-affirming :need:`REQ_0211`), with
   ``JsonCodec`` as the default codec used by example wiring
   (re-affirming :need:`REQ_0212`).

.. req:: Tokio sidecar contained inside the Zenoh connector crate
   :id: REQ_0403
   :status: implemented
   :satisfies: FEAT_0043
   :links: BB_0042, BB_0044, TEST_0314

   The Zenoh gateway shall host the ``zenoh::Session`` and all
   Zenoh async callbacks on a tokio runtime contained inside
   ``taktora-connector-zenoh``. Tokio shall not leak into
   taktora-executor's WaitSet thread (mirrors :need:`REQ_0321` and
   :need:`REQ_0258`).

.. req:: Zenoh bridge channels are bounded
   :id: REQ_0404
   :status: approved
   :satisfies: FEAT_0043

   The outbound (taktora-executor → tokio) and inbound (tokio →
   taktora-executor) bridges between the plugin and the Zenoh gateway
   sidecar shall be bounded channels with capacities configurable
   via ``ZenohConnectorOptions`` (``outbound_bridge_capacity`` and
   ``inbound_bridge_capacity``).

.. req:: Outbound bridge saturation surfaces as BackPressure
   :id: REQ_0405
   :status: approved
   :satisfies: FEAT_0043

   When the outbound bridge channel is full, ``ChannelWriter::send``
   (and any other plugin-side write entry-point that feeds the
   outbound bridge) shall return ``ConnectorError::BackPressure``
   and the gateway shall report ``ConnectorHealth::Degraded``.

.. req:: Inbound bridge saturation drops samples and signals Degraded
   :id: REQ_0406
   :status: implemented
   :satisfies: FEAT_0043
   :links: BB_0044, TEST_0306

   When the inbound bridge channel is full, the gateway shall
   (1) increment the per-channel inbound-drop counter exposed via
   ``InboundOutcome::Dropped { count }`` on the bridge's ``try_send``
   return, (2) drop the offending sample for that callback, and
   (3) emit a ``ConnectorHealth::Degraded { reason: "dropped N inbound frames" }``
   health transition when the cumulative inbound-drop count crosses
   the connector's configured ``inbound_drop_threshold`` (default 1).
   The Degraded transition is emitted at most once until the
   connector recovers to ``Up`` via the underlying stack's recovery
   path; the cumulative drop count itself is observable through every
   subsequent ``InboundOutcome::Dropped`` return.

.. req:: Zenoh zero-copy publish via iceoryx2 loan
   :id: REQ_0407
   :status: implemented
   :satisfies: FEAT_0043
   :links: IMPL_0060, TEST_0302

   ``ChannelWriter::send`` on a Zenoh channel shall publish
   envelopes via ``Publisher::loan`` so that the codec writes the
   payload directly into shared memory (re-affirms :need:`REQ_0205`).

.. req:: Zenoh gateway is byte-only on the inbound publish path
   :id: REQ_0408
   :status: implemented
   :satisfies: FEAT_0043
   :links: IMPL_0060, TEST_0302

   On the inbound leg (Zenoh peer → plugin), the gateway shall
   publish the raw payload bytes received from the Zenoh subscriber
   or reply callback onto the channel's inbound iceoryx2 service as
   a ``ConnectorEnvelope`` without invoking the channel's codec —
   codec decoding is the responsibility of the plugin-side
   ``ChannelReader::try_recv`` (symmetric with :need:`REQ_0327`).
