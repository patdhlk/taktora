Building block view
===================

arc42 §5.

The framework decomposes into five workspace crates plus reuse of two
existing taktora-executor crates. The decomposition is hierarchical: a
level-1 view shows crate-level building blocks; level-2 zooms into the
two crates that carry the most logic.

.. building-block:: taktora-connector-core
   :id: BB_0001
   :status: open
   :implements: REQ_0220, REQ_0221, REQ_0222

   Pure trait definitions and shared types. No IPC, no protocol code.
   Public surface: ``Connector`` trait, ``PayloadCodec`` trait,
   ``Routing`` marker, ``ChannelDescriptor<R, const N: usize>``,
   ``ConnectorHealth``, ``HealthEvent``, ``ReconnectPolicy``,
   ``ExponentialBackoff``, ``ConnectorError``.

.. building-block:: taktora-connector-transport-iox
   :id: BB_0002
   :status: open
   :implements: REQ_0200, REQ_0205, REQ_0206

   Concrete envelope (``ConnectorEnvelope<const N: usize>``) and
   iceoryx2-backed channel handles
   (``ChannelWriter<T, C, N>``, ``ChannelReader<T, C, N>``,
   ``ServiceFactory``). Depends on
   ``taktora-connector-core``, ``iceoryx2``, ``taktora-executor``.

.. building-block:: taktora-connector-codec
   :id: BB_0003
   :status: open
   :implements: REQ_0210, REQ_0212, REQ_0215, REQ_0989

   Concrete ``PayloadCodec`` implementations. ``JsonCodec`` ships
   default-on; ``BinaryCodec`` (fixed-width, selectable-endianness,
   ``bincode``-backed) ships behind the opt-in ``binary`` feature;
   ``MsgPackCodec`` (``rmp-serde``-backed MessagePack) ships behind the
   opt-in ``msgpack`` feature; ``ProtoCodec`` is deferred behind a cargo
   feature.

.. building-block:: taktora-connector-mqtt
   :id: BB_0004
   :status: open
   :implements: REQ_0250, REQ_0251, REQ_0258

   MQTT plugin (``MqttConnector<C>`` implementing ``Connector``) and
   gateway (``MqttGateway`` exposing executable items). Hosts the
   tokio sidecar driving ``rumqttc::EventLoop`` and the bridge
   between taktora-executor and tokio.

.. building-block:: taktora-connector-host
   :id: BB_0005
   :status: open
   :implements: REQ_0270, REQ_0271, REQ_0272

   Composition layer. Provides ``ConnectorHost::builder()`` and
   ``ConnectorGateway::builder()`` wrapping a
   ``taktora_executor::Executor``. Optional ``Observer`` adapter to
   ``taktora-executor-tracing`` lives behind a ``tracing`` cargo feature.

.. architecture:: Level-1 building block decomposition
   :id: ARCH_0002
   :status: open
   :refines: BB_0001, BB_0002, BB_0003, BB_0004, BB_0005, BB_0030, BB_0040

   Crate-level building blocks and their dependency graph. All edges
   point from depender to dependee. The graph is acyclic; the host is
   the only consumer of every other new crate. The
   ``taktora-connector-ethercat`` crate (BB_0030) is a peer of
   ``taktora-connector-mqtt`` (BB_0004) — both depend on the same
   core / transport / codec triad and feed the host.

   .. mermaid::

      flowchart TB
        subgraph existing_crates[existing crates]
          EX[taktora-executor]
          TR[taktora-executor-tracing]
        end
        subgraph new_crates["new crates (this spec)"]
          CO[taktora-connector-core<br/>BB_0001]
          TX[taktora-connector-transport-iox<br/>BB_0002]
          CD[taktora-connector-codec<br/>BB_0003]
          MQ[taktora-connector-mqtt<br/>BB_0004]
          EC[taktora-connector-ethercat<br/>BB_0030]
          ZE[taktora-connector-zenoh<br/>BB_0040]
          HO[taktora-connector-host<br/>BB_0005]
        end
        CO --> TX
        CO --> CD
        CO --> MQ
        CO --> EC
        TX --> MQ
        TX --> EC
        CD --> MQ
        CD --> EC
        EX --> TX
        EX --> MQ
        EX --> EC
        CO --> HO
        TX --> HO
        CD --> HO
        MQ --> HO
        EC --> HO
        CO --> ZE
        TX --> ZE
        CD --> ZE
        EX --> ZE
        ZE --> HO
        TR -.optional adapter.-> HO

.. building-block:: ConnectorEnvelope (sub-block of BB_0002)
   :id: BB_0010
   :status: open
   :implements: REQ_0200, REQ_0201, REQ_0202, REQ_0203, REQ_0204

   The on-wire form. ``#[repr(C)]`` POD type with a fixed header
   (sequence number, timestamp, length, correlation id) and a
   const-generic-sized payload buffer.

   .. code-block:: rust

      #[repr(C)]
      #[derive(Debug, Copy, Clone, ZeroCopySend)]
      pub struct ConnectorEnvelope<const N: usize> {
          pub sequence_number: u64,
          pub timestamp_ns:    u64,
          pub payload_length:  u32,
          pub _reserved:       u32,
          pub correlation_id:  [u8; 32],
          pub payload:         [u8; N],
      }

   At plan stage, the implementation may substitute a small set of
   size-tier types (4 KB / 64 KB / 1 MB) for the const-generic
   variant. The external contract — fixed at service-creation time —
   is identical either way.

.. building-block:: ServiceFactory (sub-block of BB_0002)
   :id: BB_0011
   :status: open
   :implements: REQ_0206

   Derives iceoryx2 service names deterministically from a
   ``ChannelDescriptor`` and creates the publisher / subscriber /
   event-service pairs for each direction.

   .. code-block:: text

      out service:    taktora.connector.<connector>.<channel>.out
      in  service:    taktora.connector.<connector>.<channel>.in
      out event:      taktora.connector.<connector>.<channel>.out.evt
      in  event:      taktora.connector.<connector>.<channel>.in.evt

.. building-block:: MqttConnector (sub-block of BB_0004, plugin side)
   :id: BB_0020
   :status: open
   :implements: REQ_0250, REQ_0251

   ``MqttConnector<C: PayloadCodec>``. Implements ``Connector`` with
   ``type Routing = MqttRouting``. ``create_writer`` /
   ``create_reader`` build ``ServiceFactory``-backed channel handles;
   ``health()`` reads the gateway's status snapshot.

.. building-block:: MqttGateway (sub-block of BB_0004, gateway side)
   :id: BB_0021
   :status: open
   :implements: REQ_0258, REQ_0259, REQ_0260, REQ_0261

   Hosts ``rumqttc::AsyncClient`` + ``EventLoop`` on a tokio runtime,
   plus the bridge channels and the executable items
   (``OutboundGatewayItem``, ``InboundGatewayItem``) registered with
   taktora-executor.

.. building-block:: Tokio bridge (sub-block of BB_0021)
   :id: BB_0022
   :status: open
   :implements: REQ_0259, REQ_0260, REQ_0261

   Two bounded channel pairs that translate between taktora-executor's
   thread (WaitSet driver) and the tokio runtime owning rumqttc.
   Outbound = ``tokio::sync::mpsc``; inbound = ``crossbeam_channel``
   wired as a taktora-executor signal source.

.. building-block:: taktora-connector-ethercat
   :id: BB_0030
   :status: open
   :implements: REQ_0310, REQ_0311, REQ_0312, REQ_0321

   EtherCAT plugin (``EthercatConnector<C>`` implementing
   ``Connector``) and gateway (``EthercatGateway`` exposing executable
   items). Hosts the tokio sidecar driving ethercrab's ``tx_rx_task``
   and the bridge between taktora-executor and tokio. Depends on
   ``taktora-connector-core``, ``taktora-connector-transport-iox``,
   ``ethercrab``, ``taktora-executor``.

.. building-block:: EthercatConnector (sub-block of BB_0030, plugin side)
   :id: BB_0031
   :status: open
   :implements: REQ_0310, REQ_0311

   Plugin-side ``EthercatConnector<C: PayloadCodec>``. Owns no I/O —
   produces ``ChannelWriter`` / ``ChannelReader`` handles whose
   ``EthercatRouting`` (SubDevice configured address, PDO direction,
   bit offset within the SubDevice's process data, bit length of the
   mapped object) identifies one process-data slice. Acts as a
   compile-time-checked façade over the gateway's SHM services.

.. building-block:: EthercatGateway (sub-block of BB_0030, gateway side)
   :id: BB_0032
   :status: open
   :implements: REQ_0312, REQ_0313, REQ_0325

   Gateway-side executable item that owns the ethercrab ``MainDevice``
   and ``PduStorage`` on one Linux network interface. Brings the bus
   from INIT through PRE-OP and SAFE-OP to OP via the typestate
   ``init_single_group`` / ``into_op`` API before serving plugin
   traffic. Opens the NIC via ``ethercrab::std::tx_rx_task``;
   requires ``CAP_NET_RAW``.

.. building-block:: PDO mapping (sub-block of BB_0030)
   :id: BB_0033
   :status: open
   :implements: REQ_0314, REQ_0315

   Module that accepts a static PDO-mapping description per SubDevice
   from ``EthercatConnectorOptions`` and applies it via SDO writes to
   the sync-manager assignment indices ``0x1C12`` (RxPDO) and
   ``0x1C13`` (TxPDO) during the PRE-OP → SAFE-OP transition. No ESI
   or EEPROM parsing.

.. building-block:: Tokio bridge for ethercrab (sub-block of BB_0030)
   :id: BB_0034
   :status: open
   :implements: REQ_0322, REQ_0323, REQ_0324

   Two bounded channel pairs that translate between taktora-executor's
   WaitSet thread and the tokio runtime owning ethercrab's
   ``tx_rx_task``. Outbound saturation surfaces as
   ``ConnectorError::BackPressure`` plus ``ConnectorHealth::Degraded``;
   inbound saturation emits ``HealthEvent::DroppedInbound { count }``
   and drops the inbound process image for the affected cycle.

.. building-block:: taktora-connector-zenoh
   :id: BB_0040
   :status: open
   :implements: REQ_0400, REQ_0420, REQ_0440, REQ_0444

   Zenoh plugin (``ZenohConnector<C>`` implementing ``Connector``)
   and gateway (``ZenohGateway`` exposing executable items). Hosts
   the tokio sidecar driving ``zenoh::Session`` and the bridge
   between taktora-executor and tokio. Depends on
   ``taktora-connector-core``, ``taktora-connector-transport-iox``,
   ``taktora-connector-codec``, ``taktora-executor``, and (behind the
   ``zenoh-integration`` feature) ``zenoh``.

.. building-block:: ZenohConnector (sub-block of BB_0040, plugin side)
   :id: BB_0041
   :status: open
   :implements: REQ_0400, REQ_0401, REQ_0420

   Plugin-side ``ZenohConnector<C: PayloadCodec>``. Implements
   ``Connector`` with ``type Routing = ZenohRouting`` and adds
   concrete non-trait methods ``create_querier`` /
   ``create_queryable``. Owns no I/O — produces ``ChannelWriter`` /
   ``ChannelReader`` / ``ZenohQuerier`` / ``ZenohQueryable`` handles
   whose ``ZenohRouting`` identifies a Zenoh key expression and the
   pub/sub QoS knobs. Acts as a compile-time-checked façade over
   the gateway's SHM services.

.. building-block:: ZenohGateway (sub-block of BB_0040, gateway side)
   :id: BB_0042
   :status: open
   :implements: REQ_0403, REQ_0426, REQ_0440, REQ_0442

   Gateway-side executable item that owns one ``zenoh::Session``
   created via ``zenoh::open(config)`` (or a ``MockZenohSession``
   when ``zenoh-integration`` is off — both implement the
   ``ZenohSessionLike`` trait). Maintains a per-channel routing
   registry mapping each open ``ChannelDescriptor`` to its
   declared Zenoh primitive (publisher / subscriber / queryable),
   and a ``correlation_id → zenoh::Query`` map for in-flight
   queryable reply streams. Translates session-alive ↔
   session-closed transitions into ``HealthEvent``s without
   using ``ReconnectPolicy``.

.. building-block:: Zenoh query handles (sub-block of BB_0041)
   :id: BB_0043
   :status: open
   :implements: REQ_0420, REQ_0421, REQ_0422, REQ_0423, REQ_0424

   ``ZenohQuerier<Q, R, C, N>`` and ``ZenohQueryable<Q, R, C, N>``.
   The non-trait query handle types. ``ZenohQuerier::send`` mints
   a ``QueryId``, encodes ``Q`` via the connector's codec, and
   publishes on the channel's ``{name}.query.out`` iceoryx2
   service; ``try_recv`` drains ``{name}.reply.in`` and decodes
   the 1-byte frame discriminator (0x01=data, 0x02=EoS,
   0x03=timeout) plus the codec-encoded ``R`` chunk.
   ``ZenohQueryable::try_recv`` surfaces ``(QueryId, Q)``; ``reply``
   stamps the ``QueryId`` back onto a reply envelope and publishes
   on ``{name}.reply.out``; ``terminate(id)`` publishes a 0x02
   envelope finalising the upstream ``zenoh::Query``.

.. building-block:: Tokio bridge for zenoh (sub-block of BB_0042)
   :id: BB_0044
   :status: open
   :implements: REQ_0403, REQ_0404, REQ_0405, REQ_0406

   Two bounded channel pairs that translate between taktora-executor's
   WaitSet thread and the tokio runtime owning ``zenoh::Session``.
   Outbound saturation surfaces as ``ConnectorError::BackPressure``
   plus ``ConnectorHealth::Degraded``; inbound saturation emits
   ``HealthEvent::DroppedInbound { count }`` and drops the
   offending sample or reply chunk. Same shape as :need:`BB_0034`
   (EtherCAT) and :need:`BB_0022` (MQTT).

.. building-block:: taktora-connector-can crate
   :id: BB_0070
   :status: open
   :implements: REQ_0600, REQ_0602, REQ_0603, REQ_0604, REQ_0605

   CAN plugin (``CanConnector<C>`` implementing ``Connector``) and
   gateway (``CanGateway`` exposing executable items). Hosts the
   tokio sidecar driving N SocketCAN sockets and the bridges
   between taktora-executor and tokio. Depends on
   ``taktora-connector-core``, ``taktora-connector-transport-iox``,
   ``taktora-connector-codec``, ``taktora-executor``, and (behind the
   ``socketcan-integration`` feature) ``socketcan`` with its
   ``tokio`` feature enabled. Ships ``MockCanInterface``
   unfeature-gated for layer-1 tests on any host OS.

.. building-block:: CanConnector (sub-block of BB_0070, plugin side)
   :id: BB_0071
   :status: open
   :implements: REQ_0600, REQ_0601, REQ_0612, REQ_0615, REQ_0621

   Plugin-side ``CanConnector<C: PayloadCodec>``. Implements
   ``Connector`` with ``type Routing = CanRouting``. Owns no I/O —
   produces ``ChannelWriter<T, C, N>`` / ``ChannelReader<T, C, N>``
   handles whose ``CanRouting`` declares the target interface,
   CAN ID, mask, frame kind, and FD flags. Validates that
   ``CanRouting::iface`` belongs to the configured gateway's
   interface set and that ``ChannelDescriptor::max_payload_size``
   matches ``CanRouting::kind`` (8 for Classical, 64 for FD)
   before any iceoryx2 service is created. Acts as a
   compile-time-checked façade over the gateway's SHM services.

.. building-block:: CanGateway (sub-block of BB_0070, gateway side)
   :id: BB_0072
   :status: open
   :implements: REQ_0613, REQ_0614, REQ_0620, REQ_0624, REQ_0625, REQ_0630, REQ_0631

   Gateway-side executable item that owns one ``CanInterfaceLike``
   per configured interface (real ``socketcan::CanSocket`` /
   ``CanFdSocket`` when ``socketcan-integration`` is on,
   ``MockCanInterface`` otherwise — both implement
   ``CanInterfaceLike``). For each interface, runs an RX task
   draining the socket and a TX drain consuming the outbound
   bridge. Maintains a per-interface routing registry mapping
   each open ``ChannelDescriptor`` to its ``CanRouting`` and
   direction. Aggregates per-interface sub-states into the
   externally-visible ``ConnectorHealth`` via worst-of
   (:need:`REQ_0630`), enables ``CAN_ERR_FLAG`` on every owned
   socket, classifies error frames internally (:need:`REQ_0631`),
   and never forwards error frames to plugin channels
   (:need:`REQ_0636`, :need:`REQ_0643`).

.. building-block:: Tokio bridge for CAN (sub-block of BB_0072)
   :id: BB_0073
   :status: open
   :implements: REQ_0605, REQ_0606, REQ_0607, REQ_0608

   Two bounded channel pairs per owned interface that translate
   between taktora-executor's WaitSet thread and the tokio runtime
   owning the SocketCAN sockets. Outbound saturation surfaces as
   ``ConnectorError::BackPressure`` plus
   ``ConnectorHealth::Degraded``; inbound saturation emits
   ``HealthEvent::DroppedInbound { count }`` and drops the
   offending CAN frame. Same shape as :need:`BB_0044` (Zenoh),
   :need:`BB_0034` (EtherCAT), and :need:`BB_0022` (MQTT).

.. building-block:: Per-iface filter compiler (sub-block of BB_0072)
   :id: BB_0074
   :status: open
   :implements: REQ_0622, REQ_0623, REQ_0624

   Pure-logic helper that maps the per-interface registry of
   inbound ``CanRouting`` entries to a single
   ``Vec<libc::can_filter>`` (or the ``socketcan`` crate's
   equivalent newtype) and applies it via
   ``setsockopt(SOL_CAN_RAW, CAN_RAW_FILTER, …)``. Recomputed
   whenever a reader is created or dropped on the affected
   interface; the recompute does not require the socket to be
   re-opened or the bus to leave its current state. Symmetric
   counterpart for the inbound demux side: given a received
   frame, returns the list of registered readers whose
   ``(can_id, mask, extended)`` matches under kernel
   ``CAN_RAW_FILTER`` semantics so that every matching reader
   gets its own envelope copy (:need:`REQ_0624`).

.. building-block:: MockCanInterface (sub-block of BB_0070)
   :id: BB_0075
   :status: open
   :implements: REQ_0604

   In-process loopback implementation of ``CanInterfaceLike``,
   shipping in the default build (not gated by
   ``socketcan-integration``). Sends queued for transmission on a
   mock interface are immediately delivered to any reader whose
   filter matches; programmable error-frame injection drives the
   :need:`BB_0072` gateway's health classifier under test.
   Exists so the Layer-1 test pyramid can exercise the full
   envelope ↔ interface ↔ envelope hop on Linux, macOS, and
   Windows without depending on the real ``socketcan`` crate or
   a Linux kernel CAN module. Mirrors :need:`BB_0040`'s
   ``MockZenohSession`` posture under :need:`REQ_0445`.

.. building-block:: taktora-connector-ui-contract crate
   :id: BB_0045
   :status: open
   :implements: REQ_0857, REQ_0869, REQ_0873, REQ_0874, REQ_0875

   The language-neutral schema shared by server and client so they
   cannot disagree — its JSON *is* the cross-language spec. Defines the
   ``Manifest`` / ``ViewModelSchema`` / ``CommandSchema`` /
   ``FieldSchema`` types, the closed ``FieldType`` descriptor set
   (scalars, fixed arrays, inline bounded UTF-8 strings, nested
   ``Struct``, C-like enums), the ``Kind`` discriminant
   (``Property`` | ``Command`` | ``CanExecute``, reserving ``Event``),
   the closed ``RejectedCode`` reason set, the ``Ack`` reply shape, and
   the canonical ``contract_hash`` algorithm. Depends on nothing in the
   connector stack; pure ``serde``. No executor, no iceoryx2.

.. building-block:: taktora-connector-ui crate (server)
   :id: BB_0046
   :status: open
   :implements: REQ_0855, REQ_0856, REQ_0857, REQ_0860, REQ_0861, REQ_0862, REQ_0863, REQ_0865, REQ_0866, REQ_0867, REQ_0870, REQ_0871, REQ_0872, REQ_0879, REQ_0883, REQ_0884

   The server side: ``UiConnector<C: PayloadCodec>`` implementing the
   shared ``Connector`` trait with ``type Routing = UiRouting`` and the
   MVVM ergonomics layer (``Property`` / ``CanExecute`` / command
   authoring) desugared onto ``create_writer`` / ``create_reader``.
   Hosts the per-ViewModel seqlock latest-value cells written on the
   (possibly RT) hot path (:need:`REQ_0860`), the non-RT publisher
   ``Pump`` that snapshots / JSON-encodes / publishes at a configurable
   cadence with coalescing and zero-subscriber skip
   (:need:`REQ_0861`, :need:`REQ_0862`), the off-RT ``CommandHandler``
   with its bounded effect channel, ``correlation_id`` LRU dedupe and
   acceptance-ack replies (:need:`REQ_0865`–:need:`REQ_0871`), the
   manifest publisher (:need:`REQ_0872`), the mandatory
   ``SystemViewModel`` heartbeat with process epoch (:need:`REQ_0879`),
   the hot-scalar promotion API (``add_hot_scalar`` —
   :need:`REQ_0863`), and the local-publish health state machine
   (:need:`REQ_0883`). Trust is OS- and iceoryx2-mediated
   (:need:`REQ_0884`); the crate exposes no authentication surface.
   Depends on ``taktora-connector-core``,
   ``taktora-connector-transport-iox``,
   ``taktora-connector-ui-contract``, and ``taktora-executor``.

.. building-block:: taktora-connector-ui-derive crate
   :id: BB_0047
   :status: open
   :implements: REQ_0858, REQ_0859, REQ_0868, REQ_0878

   The ``#[derive(ViewModel)]`` / ``#[derive(CommandParams)]`` /
   ``#[derive(ImageEnum)]`` proc-macros and the ``#[command(idempotent)]``
   attribute. From the authored Rust type the macro computes the
   compile-time maximum encoded size and instantiates
   ``ConnectorEnvelope<N>`` (:need:`REQ_0859`), emits the manifest
   contribution — field names, ``FieldType`` schema descriptors,
   command signatures, kinds, and idempotent flags (:need:`REQ_0878`),
   and enforces the closed POD field-type set by rejecting heap-backed
   types, data-carrying unions, and 128-bit integers at compile time
   with a clear diagnostic (:need:`REQ_0858`). Nested-struct fields are
   not yet generated (the deferred slice of :need:`REQ_0858`): they
   currently land on a purposeful "not yet supported" ``compile_error!``
   rather than silently mis-encoding.

.. building-block:: taktora-connector-ui-client crate
   :id: BB_0048
   :status: open
   :implements: REQ_0864, REQ_0876, REQ_0877, REQ_0880, REQ_0881, REQ_0882

   The Rust reference consumer (no executor dependency). Discovers live
   applications by scanning the iceoryx2 service registry for the
   manifest naming pattern (:need:`REQ_0877`), reads and hash-validates
   the manifest, binding read-write on a match and entering the
   read-only inspect fallback (commands disabled) on a mismatch
   (:need:`REQ_0876`). Reconstructs per-field ``PropertyChanged`` by
   diffing each received ViewModel against the last held copy
   (:need:`REQ_0864`) and computes per-ViewModel staleness from the
   envelope ``timestamp_ns`` / ``sequence_number`` (:need:`REQ_0880`).
   Recovers statelessly on UI restart via history-depth-1 redelivery
   (:need:`REQ_0881`) and re-reads / re-validates the manifest on an
   epoch change (:need:`REQ_0882`).

.. building-block:: Large-payload slice channel (transport-iox)
   :id: BB_0097
   :status: implemented
   :implements: REQ_0885, REQ_0886, REQ_0887, REQ_0888, REQ_0889

   An additive transport in ``taktora-connector-transport-iox`` beside
   :need:`BB_0002`'s ``ConnectorEnvelope<N>``: a ``SliceChannelWriter`` /
   ``SliceChannelReader`` pair over an iceoryx2 slice (``[u8]``)
   publish-subscribe service. Loans are sized to the message at send time
   (``loan_slice_uninit``); the data segment starts at a configurable
   ``initial_max_slice_len`` and grows by ``AllocationStrategy::PowerOfTwo``
   up to a configurable ``max_payload_bytes`` ceiling, past which a loan is
   refused with a bounded-capacity ``ConnectorError`` so growth stays
   auditable (:need:`REQ_0888`). ``sequence_number`` / ``timestamp_ns``
   ride an iceoryx2 user-header rather than an inline POD struct
   (:need:`REQ_0889`). First consumer: J1939 ETP (:need:`BB_0101`).

.. building-block:: taktora-connector-j1939 crate
   :id: BB_0098
   :status: implemented
   :implements: REQ_0890, REQ_0899

   J1939 plugin (``J1939Connector<C>`` implementing ``Connector``) and
   gateway (``J1939Gateway`` exposing executable items), layering SAE
   J1939 on CAN. Depends on ``taktora-connector-core``,
   ``taktora-connector-transport-iox`` (envelope **and**
   :need:`BB_0097` slice channel), ``taktora-connector-codec``,
   ``taktora-executor``, and — at the crate level — ``taktora-connector-can``
   for its ``CanInterfaceLike`` driver layer (``MockCanInterface`` /
   feature-gated ``RealCanInterface``, ``CanData`` / ``CanFrame``,
   ``CanGateway``). Hosts the tokio sidecar driving the TP state machine
   (:need:`BB_0101`) and the address-claim state machine
   (:need:`BB_0102`). Reuses the CAN driver and owns its own
   PGN-aware dispatcher per :need:`ADR_0108`.

.. building-block:: J1939Connector (sub-block of BB_0098, plugin side)
   :id: BB_0099
   :status: implemented
   :implements: REQ_0890, REQ_0891

   Plugin-side ``J1939Connector<C: PayloadCodec>``. Implements
   ``Connector`` with ``type Routing = J1939Routing``. Owns no I/O —
   produces ``ChannelWriter`` / ``ChannelReader`` handles whose
   ``J1939Routing`` declares PGN, optional source/destination-address
   filters, transport class (``SingleFrame`` | ``Tp { max_len }``), and TX
   priority. Validates that the channel's ``N`` matches the declared
   transport class before any iceoryx2 service is created
   (:need:`REQ_0891`), mirroring :need:`BB_0071`'s frame-kind check.
   Routes bounded traffic onto ``ConnectorEnvelope<N>`` channels and ETP
   onto the :need:`BB_0097` slice channel.

.. building-block:: J1939Gateway (sub-block of BB_0098, gateway side)
   :id: BB_0100
   :status: implemented
   :implements: REQ_0890, REQ_0895, REQ_0896, REQ_0898

   Gateway-side executable item that owns one ``CanInterfaceLike`` per
   configured interface (reused from :need:`BB_0072`'s driver layer) and
   runs a PGN-aware dispatcher: decode the 29-bit ID into priority / PDU
   format / PGN / SA / DA, demux single-frame PGNs straight to matching
   channels, and feed multi-packet frames into the TP state machine
   (:need:`BB_0101`). Maintains a per-interface routing registry keyed by
   PGN with optional SA/DA filters. Aggregates TP-session and
   address-claim state into the externally-visible ``ConnectorHealth``;
   gates outbound transmission until the interface's address is Claimed
   (:need:`REQ_0898`).

.. building-block:: J1939 transport-protocol state machine (sub-block of BB_0100)
   :id: BB_0101
   :status: implemented
   :implements: REQ_0892, REQ_0893, REQ_0894, REQ_0895, REQ_0896

   Userspace TP engine covering BAM (TP.CM + TP.DT), RTS/CTS
   connection-mode (RTS / CTS / EndOfMsgAck / Abort + TP.DT), and ETP.
   Reassembles inbound and segments outbound multi-packet messages;
   enforces the J1939-21 timers (Tr, Th, T1–T4) and surfaces every
   timeout / abort as a ``HealthEvent`` (:need:`REQ_0895`). Bounds
   concurrent inbound sessions per interface to a configurable maximum,
   refusing excess sessions with a connection abort (:need:`REQ_0896`).
   BAM / RTS-CTS payloads (≤ 1785 B) land on ``ConnectorEnvelope<N>``
   channels; ETP payloads land on the :need:`BB_0097` slice channel,
   bounded by ``max_etp_bytes`` (:need:`REQ_0894`, :need:`ADR_0109`).

.. building-block:: J1939 address-claim state machine (sub-block of BB_0100)
   :id: BB_0102
   :status: implemented
   :implements: REQ_0897, REQ_0898

   J1939-81 address manager, one instance per owned interface. Claims a
   configured source address using a 64-bit NAME, arbitrates by NAME
   priority on contention, falls back to the null address (254) as
   cannot-claim, responds to Request-for-Address-Claimed (PGN 59904), and
   honours Address-Commanded (PGN 65240). Drives the interface's
   ``Claiming → Claimed | CannotClaim`` state, mapped onto
   ``ConnectorHealth`` (``Connecting`` / ``Up`` / ``Down``) by
   :need:`BB_0100`, which gates TX until Claimed.

.. building-block:: MockJ1939Interface (sub-block of BB_0098)
   :id: BB_0103
   :status: implemented
   :implements: REQ_0899

   In-process implementation harness for the layer-1 test pyramid, built
   on :need:`BB_0075`'s ``MockCanInterface``. Lets tests inject raw CAN
   frames (BAM / RTS-CTS / ETP sequences, AC traffic) and observe the
   reassembled PGN payloads and claim transitions deterministically on any
   host OS, without a Linux kernel CAN module — mirroring how
   :need:`BB_0075` exercises the CAN gateway under :need:`REQ_0604`.
