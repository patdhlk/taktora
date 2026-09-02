CAN (SocketCAN) reference connector
====================================

Verification artefacts for the CAN reference connector. Layer-1
(pure-logic) cases use ``MockCanInterface``; layer-2 cases gated on
``socketcan-integration`` require a Linux host with the ``vcan``
kernel module loaded (``modprobe vcan && ip link add dev vcan0 type
vcan && ip link set up vcan0``).

.. test:: CanConnector trait surface
   :id: TEST_0500
   :status: implemented
   :verifies: REQ_0600

   Compile-time API surface check that ``CanConnector<C>`` implements
   the framework ``Connector`` trait with ``type Routing =
   CanRouting`` and ``type Codec = C``. Asserts ``create_writer<T>``
   and ``create_reader<T>`` return the framework's concrete
   ``ChannelWriter<T, C, N>`` / ``ChannelReader<T, C, N>`` (not boxed
   trait objects, mirroring :need:`REQ_0223`). Realised as
   ``crates/taktora-connector-can/tests/connector_surface.rs``.

.. test:: CanRouting field round-trip
   :id: TEST_0501
   :status: open
   :verifies: REQ_0601, REQ_0615

   Property test (``proptest``) generating arbitrary
   ``CanRouting { iface, can_id, mask, kind, fd_flags }`` and
   asserting clone / equality / debug round-trip. Asserts that
   ``CanId::standard(0x123)`` and ``CanId::extended(0x123)`` are
   distinct values (the ``extended`` flag is part of identity) and
   that 11-bit standard IDs reject construction above ``0x7FF``,
   29-bit extended IDs reject construction above ``0x1FFF_FFFF``.

.. test:: Classical CAN round-trip via MockCanInterface
   :id: TEST_0502
   :status: implemented
   :verifies: REQ_0610, REQ_0612, REQ_0613, REQ_0614

   Layer-1 end-to-end test: ``CanConnector`` over
   ``MockCanInterface`` with one outbound and one inbound channel
   on a single mock iface, both ``CanFrameKind::Classical``. Send
   100 envelopes carrying 1–8 byte payloads with both standard
   and extended IDs; assert each is delivered byte-for-byte to
   the inbound reader and that ``CanFrame`` DLC matches the
   encoded payload length.

.. test:: CAN-FD round-trip via MockCanInterface
   :id: TEST_0503
   :status: implemented
   :verifies: REQ_0611, REQ_0613

   As :need:`TEST_0502` but with ``CanFrameKind::Fd`` channels
   carrying payloads at the FD DLC steps (8, 12, 16, 20, 24, 32,
   48, 64 bytes). Asserts BRS and ESI flags from
   ``CanRouting::fd_flags`` are preserved on the mock interface
   and surface on the receiving side.

.. test:: Per-iface filter union
   :id: TEST_0504
   :status: implemented
   :verifies: REQ_0622, REQ_0623

   Open three inbound readers on the same mock iface with
   distinct ``(can_id, mask)`` pairs; assert that ``BB_0074``'s
   filter compiler emits exactly three ``can_filter`` entries
   whose union covers all three readers. Drop one reader and
   assert the filter is recomputed and re-applied without
   reopening the underlying interface (mock interface records
   ``apply_filter`` calls).

.. test:: Multi-iface inbound demux
   :id: TEST_0505
   :status: implemented
   :verifies: REQ_0620, REQ_0621, REQ_0624

   Gateway owns two mock ifaces (``vcan0``, ``vcan1``). Open one
   reader on ``vcan0`` for ``can_id = 0x100`` and two readers on
   ``vcan1`` for ``(0x200, mask=0x7F0)`` (overlapping). Inject
   frames on each iface and assert: ``vcan0`` reader sees only
   its own frame; both ``vcan1`` readers see their matching
   frames; readers never see frames from the other iface; the
   two overlapping ``vcan1`` readers each receive their own
   envelope copy when an ID lands in their intersection.

.. test:: Bus-off → Down → ReconnectPolicy reopen
   :id: TEST_0506
   :status: implemented
   :verifies: REQ_0633, REQ_0634

   Drive ``MockCanInterface`` through ``Connecting → Up`` then
   inject a bus-off error frame. Assert: the affected iface
   transitions to ``Down``, its socket is closed,
   ``ReconnectPolicy::next_delay()`` is consulted, a reopen
   attempt is scheduled, the filter is re-applied
   (:need:`REQ_0623`), and the sub-state walks back through
   ``Connecting → Up``. Uses a deterministic stub
   ``ReconnectPolicy`` returning fixed 1 ms delays so the test
   runs without sleeping.

.. test:: error-passive → Degraded → recovery
   :id: TEST_0507
   :status: implemented
   :verifies: REQ_0630, REQ_0632, REQ_0635

   Two-iface gateway. Inject an error-passive error frame on
   ``vcan0`` while ``vcan1`` remains ``Up``. Assert: ``vcan0``'s
   sub-state transitions to ``Degraded``; the connector's
   aggregated ``ConnectorHealth`` surfaces as ``Degraded`` per
   :need:`REQ_0630`; one ``HealthEvent`` is emitted carrying
   ``DegradedReason::ErrorPassive { iface: "vcan0" }``. After
   the configured ``recovery_window`` elapses with no further
   error frames, the sub-state returns to ``Up`` and a second
   ``HealthEvent`` is emitted.

.. test:: Tokio sidecar contained inside taktora-connector-can
   :id: TEST_0508
   :status: open
   :verifies: REQ_0605

   Static check that the SocketCAN sockets and any tokio runtime
   handle live entirely inside the ``taktora-connector-can`` crate.
   No public type exported by ``taktora-connector-can`` shall name a
   ``tokio::*`` or ``socketcan::*`` type in its signature
   (compile-time API surface scan). Realised as
   ``crates/taktora-connector-can/tests/tokio_containment.rs``,
   shelling out to ``cargo public-api`` and asserting absence of
   ``tokio::`` and (when ``socketcan-integration`` is enabled)
   ``socketcan::`` identifiers in the public surface. Mirrors
   :need:`TEST_0314`.

.. test:: Outbound bridge saturation surfaces as BackPressure
   :id: TEST_0509
   :status: open
   :verifies: REQ_0606, REQ_0607

   With ``outbound_bridge_capacity = 1`` and a deliberately
   stalled ``MockCanInterface`` (TX never drains), the second
   ``ChannelWriter::send`` returns
   ``ConnectorError::BackPressure`` and the connector's
   ``health()`` snapshot transitions to ``Degraded``.

.. test:: Inbound bridge saturation surfaces as DroppedInbound
   :id: TEST_0510
   :status: open
   :verifies: REQ_0606, REQ_0608

   With ``inbound_bridge_capacity = 1`` and a deliberately
   stalled plugin reader, the gateway emits
   ``HealthEvent::DroppedInbound { count = N }`` reflecting the
   number of CAN frames discarded. Subsequent reader drains
   resume normally.

.. test:: socketcan-integration feature gates the real socketcan dep
   :id: TEST_0511
   :status: implemented
   :verifies: REQ_0603, REQ_0604
   :links: BB_0070, IMPL_0080

   Build the crate twice — once with default features, once with
   ``--features socketcan-integration`` — and assert that the
   default build does not link ``socketcan`` (via ``cargo tree``
   introspection) while the feature build does. Both builds
   expose ``MockCanInterface``. Realised as
   ``scripts/check_dep_gating_can.sh`` invoked from the
   ``dep-gating`` job in ``.github/workflows/ci-can.yml``. The
   feature-build assertion runs on Linux only — the ``socketcan``
   dep is declared under a ``cfg(target_os = "linux")`` target
   table per :need:`REQ_0502`, so non-Linux hosts legitimately
   omit it; the script reports a skip notice in that case.

.. test:: Linux raw-socket smoke against vcan0
   :id: TEST_0512
   :status: implemented
   :verifies: REQ_0502, REQ_0613, REQ_0614
   :links: BB_0070, IMPL_0080

   Layer-2 integration test (``socketcan-integration`` feature,
   Linux only): require ``vcan0`` present (``modprobe vcan && ip
   link add dev vcan0 type vcan && ip link set up vcan0``). Open
   two ``RealCanInterface`` instances bound to ``vcan0`` (one
   sending, one receiving, exploiting PF_CAN raw-socket broadcast
   semantics), apply an accept-all filter on the rx side, send
   10 classical frames carrying small distinct payloads, and
   assert each round-trips byte-for-byte with the correct CAN
   identifier and ``extended`` flag. Realised as
   ``crates/taktora-connector-can/tests/vcan_smoke.rs``, gated
   ``#[ignore]`` so plain ``cargo test`` skips it; the
   ``vcan-smoke`` job in ``.github/workflows/ci-can.yml`` runs
   it with ``--include-ignored`` after bringing up ``vcan0``.

.. test:: Error frames not exposed to plugin
   :id: TEST_0513
   :status: implemented
   :verifies: REQ_0631, REQ_0636, REQ_0643

   Regression-guard for the explicit anti-requirement
   :need:`REQ_0643`. Inject error frames of every classified
   kind (error-warning, error-passive, bus-off) via
   ``MockCanInterface`` and assert no ``ChannelReader<T>`` on the
   plugin side observes a ``Received<T>`` for any of them.
   Health-channel observation is the only surface.

.. test:: Per-iface routing registry has stable iteration order
   :id: TEST_0514
   :status: implemented
   :verifies: REQ_0625

   Add 8 channels to the same mock iface in a known order; assert
   the RX dispatch loop and the TX drain loop iterate them in
   that insertion order on every cycle. Assert no per-cycle heap
   allocation: instrument a ``CountingAllocator`` and assert zero
   allocations from the RX/TX hot path over 1000 iterations
   (mirrors :need:`TEST_0219`).
