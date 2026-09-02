CAN (SocketCAN) reference connector
===================================

A fourth concrete connector instantiating the framework's contracts. This
parent feature ``:satisfies:`` :need:`FEAT_0030`; its three capability
clusters — frame transport (:need:`FEAT_0047`), the multi-interface gateway
(:need:`FEAT_0048`), and bus health, error frames, and reconnect
(:need:`FEAT_0049`) — each ``:satisfies:`` it, on their own pages (see the
toctree). The connector-wide requirements below ``:satisfies:`` the parent
feature directly.

.. feat:: CAN (SocketCAN) reference connector
   :id: FEAT_0046
   :status: open
   :satisfies: FEAT_0030

   A fourth concrete connector instantiating the framework's
   contracts: ``socketcan``-backed CAN plugin and gateway exchanging
   classical CAN and CAN-FD frames on one or more Linux SocketCAN
   network interfaces, with internal error-frame-driven health,
   ``ReconnectPolicy``-driven bus-off recovery, and a non-Linux
   ``MockCanInterface`` for layer-1 tests. The gateway owns N
   ``socketcan::CanSocket`` / ``CanFdSocket`` instances — one per
   registered interface — and runs the RX/TX loops on a tokio
   sidecar contained inside ``taktora-connector-can``. Linux is the
   only supported host OS for real I/O; the in-process mock
   interface is portable.

.. toctree::
   :maxdepth: 2

   can-frame-transport
   can-multi-interface-gateway
   can-bus-health

.. req:: CanConnector implements Connector
   :id: REQ_0600
   :status: implemented
   :satisfies: FEAT_0046
   :links: BB_0070, BB_0071, IMPL_0080, TEST_0500

   The connector crate shall expose ``CanConnector<C: PayloadCodec>``
   that implements the ``Connector`` trait with
   ``type Routing = CanRouting`` and ``type Codec = C``.

.. req:: CanRouting carries iface, can_id, mask, kind, fd_flags
   :id: REQ_0601
   :status: approved
   :satisfies: FEAT_0046

   The ``CanRouting`` struct shall identify one channel by Linux
   network interface name (``iface``, bounded ASCII string of
   ``IFNAMSIZ`` − 1 = 15 bytes), CAN identifier (``can_id``, with
   an explicit ``extended: bool`` flag distinguishing 11-bit from
   29-bit IDs), kernel-style ID mask (``mask: u32``), frame kind
   (``CanFrameKind::{Classical, Fd}``), and FD bit-rate-switch /
   error-state-indicator flags (``fd_flags: CanFdFlags``, ignored
   when ``kind == Classical``). It shall implement the ``Routing``
   marker trait.

.. req:: Linux is the supported host OS for real I/O
   :id: REQ_0602
   :status: open
   :satisfies: FEAT_0046

   The CAN gateway shall open SocketCAN interfaces via the Linux
   ``PF_CAN`` socket family, requiring the ``CAP_NET_RAW``
   capability on the gateway process (mirrors :need:`REQ_0325`).
   The plugin-side ``CanConnector`` and the ``MockCanInterface``
   shall remain portable to macOS and Windows for layer-1
   development.

.. req:: socketcan-integration cargo feature gates the real socketcan dep
   :id: REQ_0603
   :status: implemented
   :satisfies: FEAT_0046
   :links: BB_0070, IMPL_0080, TEST_0511

   The ``socketcan`` crate shall be an optional dependency of
   ``taktora-connector-can``, activated only by a default-off
   ``socketcan-integration`` cargo feature (mirrors
   :need:`REQ_0444`'s ``zenoh-integration`` posture and
   :need:`BB_0030`'s ``bus-integration`` posture).

.. req:: MockCanInterface ships unfeature-gated
   :id: REQ_0604
   :status: implemented
   :satisfies: FEAT_0046
   :links: BB_0070, BB_0075, IMPL_0080, TEST_0511

   ``MockCanInterface`` — an in-process loopback implementation of
   the ``CanInterfaceLike`` trait — shall ship in the default
   build, not gated by ``socketcan-integration``. It exists so
   that the Layer-1 (pure-logic) test pyramid can exercise the
   full envelope ↔ interface ↔ envelope hop without depending on
   the real ``socketcan`` crate or a Linux kernel CAN module
   (mirrors :need:`REQ_0445`).

.. req:: Tokio sidecar contained inside the CAN connector crate
   :id: REQ_0605
   :status: approved
   :satisfies: FEAT_0046

   The CAN gateway shall host its RX/TX tasks on a tokio runtime
   contained inside ``taktora-connector-can``. Tokio shall not leak
   into taktora-executor's WaitSet thread (mirrors :need:`REQ_0321`,
   :need:`REQ_0258`, :need:`REQ_0403`).

.. req:: CAN bridge channels are bounded
   :id: REQ_0606
   :status: approved
   :satisfies: FEAT_0046

   The outbound (taktora-executor → tokio) and inbound (tokio →
   taktora-executor) bridges between the plugin and the CAN gateway
   sidecar shall be bounded channels with capacities configurable
   via ``CanConnectorOptions`` (``outbound_bridge_capacity`` and
   ``inbound_bridge_capacity``).

.. req:: Outbound bridge saturation surfaces as BackPressure
   :id: REQ_0607
   :status: approved
   :satisfies: FEAT_0046

   When the outbound bridge channel is full, ``ChannelWriter::send``
   shall return ``ConnectorError::BackPressure`` and the gateway
   shall report ``ConnectorHealth::Degraded``.

.. req:: Inbound bridge saturation drops frames and signals Degraded
   :id: REQ_0608
   :status: open
   :satisfies: FEAT_0046

   When the inbound bridge channel is full, the gateway shall
   (1) increment the per-channel inbound-drop counter exposed via
   ``InboundOutcome::Dropped { count }`` on the bridge's ``try_send``
   return, (2) drop the offending CAN frame for that callback, and
   (3) emit a ``ConnectorHealth::Degraded { reason: "dropped N inbound frames" }``
   health transition when the cumulative inbound-drop count crosses
   the connector's configured ``inbound_drop_threshold`` (default 1).
   The Degraded transition is emitted at most once until the
   connector recovers to ``Up`` via the underlying stack's recovery
   path; the cumulative drop count itself is observable through every
   subsequent ``InboundOutcome::Dropped`` return.
