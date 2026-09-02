Zenoh queries
=============

The query half of the Zenoh connector. This cluster ``:satisfies:``
:need:`FEAT_0042`.

.. feat:: Zenoh queries
   :id: FEAT_0044
   :status: open
   :satisfies: FEAT_0042

   The query half of the Zenoh connector — Zenoh's signature
   request/response primitive, layered on top of the same
   ``ConnectorEnvelope`` shape used by pub/sub. Exposed via concrete
   non-trait methods on ``ZenohConnector``: ``create_querier`` and
   ``create_queryable``. The framework's anti-goal
   :need:`REQ_0290` (no framework-level correlation matching) is
   preserved — correlation lives inside the Zenoh-specific handle
   types, using the framework's existing 32-byte passive
   ``correlation_id`` carrier (:need:`REQ_0204`).

.. req:: ZenohConnector exposes create_querier and create_queryable
   :id: REQ_0420
   :status: implemented
   :satisfies: FEAT_0044
   :links: BB_0043, TEST_0303

   ``ZenohConnector`` shall expose, as concrete methods (NOT on the
   ``Connector`` trait), ``create_querier<Q, R, const N: usize>`` and
   ``create_queryable<Q, R, const N: usize>``, returning
   ``ZenohQuerier<Q, R, C, N>`` and ``ZenohQueryable<Q, R, C, N>``
   respectively, with ``Q`` and ``R`` bound by ``serde::Serialize`` /
   ``serde::de::DeserializeOwned`` as appropriate per direction.

.. req:: ZenohQuerier maps QueryId to envelope correlation_id
   :id: REQ_0421
   :status: implemented
   :satisfies: FEAT_0044
   :links: BB_0043, IMPL_0060, TEST_0303

   ``ZenohQuerier::send(q: Q)`` shall mint a fresh ``QueryId`` for
   each call, populate the outbound envelope's ``correlation_id``
   with the ``QueryId``, and return the ``QueryId`` to the caller so
   incoming replies on the matching ``{name}.reply.in`` iceoryx2
   service can be demultiplexed by ``QueryId``.

.. req:: ZenohQueryable correlates replies via correlation_id
   :id: REQ_0422
   :status: implemented
   :satisfies: FEAT_0044
   :links: BB_0043, TEST_0303

   ``ZenohQueryable::try_recv`` shall surface the gateway-minted
   ``QueryId`` (= the envelope's ``correlation_id``) alongside the
   decoded request value ``Q``. ``ZenohQueryable::reply(id, r)``
   shall stamp ``id`` onto the reply envelope's ``correlation_id``
   so the gateway-side dispatcher can look up the corresponding
   ``zenoh::Query`` handle. The framework shall not perform this
   matching itself (preserves :need:`REQ_0290`); the matching lives
   inside ``ZenohQueryable``.

.. req:: Multi-reply per query supported
   :id: REQ_0423
   :status: implemented
   :satisfies: FEAT_0044
   :links: BB_0043, TEST_0303

   ``ZenohQueryable::reply(id, r)`` shall be callable zero or more
   times for the same ``QueryId`` before ``terminate(id)``. Each
   call shall publish one reply envelope on the channel's
   ``{name}.reply.out`` iceoryx2 service; the gateway shall forward
   each to ``zenoh::Query::reply`` on the matching handle.

.. req:: Reply stream end-of-stream framed in payload
   :id: REQ_0424
   :status: implemented
   :satisfies: FEAT_0044
   :links: BB_0043, IMPL_0060, TEST_0303

   The end of a reply stream shall be signalled by a one-byte
   Zenoh-private frame discriminator at the start of the reply
   envelope's payload: ``0x01`` = data chunk (followed by
   codec-encoded ``R``); ``0x02`` = end of stream (no body);
   ``0x03`` = timeout terminator (gateway-synthetic, no body). The
   framework's ``ConnectorEnvelope`` reserved word
   (:need:`REQ_0200`) shall not be repurposed for this signal.
   ``ZenohQueryable::terminate(id)`` shall emit a ``0x02`` envelope
   for ``id`` and free the gateway-side ``zenoh::Query`` handle.

.. req:: Query timeout sourced from options, overridable per-querier
   :id: REQ_0425
   :status: approved
   :satisfies: FEAT_0044

   The default per-query timeout shall be sourced from
   ``ZenohConnectorOptions::query_timeout``. ``ZenohQuerier`` shall
   allow this default to be overridden at querier-creation time
   (via a builder option) or per-call (via an explicit
   ``send_with_timeout(q, timeout)`` method). Timeout expiry on the
   gateway shall emit a ``0x03`` terminator (per :need:`REQ_0424`)
   on the reply stream for that ``QueryId``.

.. req:: terminate(id) finalizes the upstream zenoh::Query
   :id: REQ_0426
   :status: implemented
   :satisfies: FEAT_0044
   :links: BB_0042, TEST_0303

   When the gateway observes a ``0x02`` end-of-stream envelope from
   the queryable side (or synthesises a ``0x03`` timeout), it shall
   drop the corresponding entry from its ``correlation_id →
   zenoh::Query`` map. Dropping the ``zenoh::Query`` handle
   finalizes the reply stream as observed by the upstream Zenoh
   peer.

.. req:: Codec applied to Q on send and to R on reply
   :id: REQ_0427
   :status: implemented
   :satisfies: FEAT_0044
   :links: IMPL_0060, TEST_0303

   ``ZenohQuerier::send`` shall encode ``Q`` via the connector's
   ``C: PayloadCodec`` into the envelope payload before SHM
   publish. ``ZenohQueryable::reply`` shall encode ``R`` via the
   same codec into ``envelope.payload[1..]`` (with byte ``[0]``
   carrying the ``0x01`` data discriminator per :need:`REQ_0424`).
   Decoding the inbound counterpart (``Q`` on the queryable side,
   ``R`` on the querier side) shall happen plugin-side in
   ``try_recv`` and shall surface codec failures as
   ``ConnectorError::Codec`` per :need:`REQ_0214`.

.. req:: Reply-side inbound saturation drops chunks and signals Degraded
   :id: REQ_0428
   :status: open
   :satisfies: FEAT_0044

   When the inbound bridge for the reply path (gateway → plugin
   on a querier channel) is full, the gateway shall
   (1) increment the per-channel inbound-drop counter exposed via
   ``InboundOutcome::Dropped { count }`` on the bridge's ``try_send``
   return, (2) drop the offending reply chunk for that callback, and
   (3) emit a ``ConnectorHealth::Degraded { reason: "dropped N inbound frames" }``
   health transition when the cumulative inbound-drop count crosses
   the connector's configured ``inbound_drop_threshold`` (default 1,
   re-affirming :need:`REQ_0406`). The Degraded transition is emitted
   at most once until the connector recovers to ``Up`` via the
   underlying stack's recovery path; the cumulative drop count itself
   is observable through every subsequent ``InboundOutcome::Dropped``
   return. The in-flight ``QueryId`` shall be observable on the
   plugin side as a reply stream with fewer chunks than the upstream
   peer sent; no separate "partial reply" error variant is added.
