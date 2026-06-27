Implementations
===============

arc42 §13.

The framework's building blocks (:need:`BB_0001`, :need:`BB_0002`,
:need:`BB_0003`, :need:`BB_0005`) and the EtherCAT reference connector
(:need:`BB_0030` and its sub-blocks) ship as five workspace crates.
Each crate has its own ``impl::`` directive recording which BB it
realises, which requirements it refines, and any deviations from the
spec text that needed amendment during implementation.

.. impl:: taktora-connector-core crate
   :id: IMPL_0010
   :status: open
   :implements: BB_0001
   :refines: REQ_0201, REQ_0210, REQ_0213, REQ_0214, REQ_0221, REQ_0222, REQ_0230, REQ_0232, REQ_0233, REQ_0234

   **Crate.** ``crates/taktora-connector-core``. No iceoryx2 or
   tokio dependency; the crate is the framework's small-types layer.
   Depends on ``thiserror``, ``serde``, ``rand`` (jitter for
   ``ExponentialBackoff``); ``proptest`` dev-only.

   **Surface.**

   * ``Routing`` marker trait (``REQ_0222``).
   * ``ChannelDescriptor<R: Routing, const N: usize>`` with
     empty-name validation (``REQ_0201``, ``REQ_0221``).
   * ``PayloadCodec`` trait — encode / decode + ``format_name``
     (``REQ_0210``). Used as a generic-parameter constraint by
     concrete connectors.
   * ``ConnectorError`` — ``Codec`` / ``BackPressure`` /
     ``PayloadOverflow`` / ``Configuration`` / ``Down`` /
     ``Stack`` (``REQ_0213``, ``REQ_0214``).
   * ``ConnectorHealth`` + ``ConnectorHealthKind`` + ``HealthEvent``
     + ``HealthMonitor`` + ``IllegalTransition`` — enforces the
     ARCH_0012 transition matrix; legal transitions emit one
     event, illegal pairs return ``IllegalTransition`` or panic in
     the panic-on-illegal helper (``REQ_0230``, ``REQ_0234``).
   * ``ReconnectPolicy`` trait + ``ExponentialBackoff`` /
     ``ExponentialBackoffBuilder`` — seedable RNG for
     deterministic tests; jitter / growth / max delay clamps at
     ``build()`` time (``REQ_0232``, ``REQ_0233``).

   **Tests.** TEST_0100 (``ExponentialBackoff`` invariants,
   proptest); TEST_0101 (state-machine transitions + illegal-pair
   rejection); TEST_0103 (``ChannelDescriptor`` validation).

.. impl:: taktora-connector-transport-iox crate
   :id: IMPL_0020
   :status: open
   :implements: BB_0002
   :refines: REQ_0200, REQ_0202, REQ_0203, REQ_0204, REQ_0205, REQ_0206, REQ_0214

   **Crate.** ``crates/taktora-connector-transport-iox``. Depends on
   ``taktora-connector-core``, ``iceoryx2``, ``serde``.

   **Surface.**

   * ``ConnectorEnvelope<const N: usize>`` — ``#[repr(C)]`` POD
     with ``ZeroCopySend`` (``REQ_0200``): sequence_number,
     timestamp_ns, 32-byte correlation_id, payload_len, reserved
     word, and inline ``[u8; N]`` payload buffer.
   * ``ChannelWriter<T, C, const N: usize>`` — typed publisher.
     ``send`` / ``send_with_correlation`` use
     ``Publisher::loan_uninit`` and a raw-pointer view of the
     inline payload array so codec writes hit shared memory
     directly with no intermediate user-side buffer (``REQ_0205``).
     Sequence numbers are claimed via ``fetch_add`` *only after*
     a successful codec encode, so failed sends do not advance
     the counter (``REQ_0202``, exercised by TEST_0125).
   * ``ChannelReader<T, C, const N: usize>`` — typed subscriber;
     ``try_recv`` surfaces codec failures as
     ``ConnectorError::Codec`` rather than silently dropping the
     envelope (``REQ_0214``).
   * ``ServiceFactory`` — opens / creates the iceoryx2 pub/sub
     service for a ``ChannelDescriptor`` (``REQ_0206``). The
     two-direction split mandated by ``REQ_0206`` (outbound vs
     inbound) is intentionally realised at the host layer
     (:need:`BB_0005`): each side constructs descriptors with
     a direction suffix, and ``ServiceFactory`` opens one
     service per descriptor.

   **Tests.** Integration tests against real iceoryx2 services:
   TEST_0120 (round-trip), TEST_0121 (sequence monotonicity),
   TEST_0122 (timestamp at send), TEST_0123 (correlation id
   verbatim + default zero), TEST_0125 (payload overflow rejection
   + no sequence advance).

.. impl:: taktora-connector-codec crate
   :id: IMPL_0030
   :status: open
   :implements: BB_0003
   :refines: REQ_0210, REQ_0212, REQ_0213, REQ_0214

   **Crate.** ``crates/taktora-connector-codec``. Re-exports
   ``PayloadCodec`` from ``taktora-connector-core``; ships
   ``JsonCodec`` behind a default-on ``json`` cargo feature
   (``REQ_0212``).

   **Surface.** ``JsonCodec`` writes directly into the
   caller-provided buffer via a tiny ``CountingWriter`` adapter
   wrapping ``serde_json::to_writer``. Buffer-too-small surfaces
   as ``ConnectorError::PayloadOverflow`` (with ``actual``
   computed on the error path via a fallback ``to_vec`` —
   the success path stays allocation-free); other serializer
   faults (non-string map keys, etc.) surface as
   ``ConnectorError::Codec`` carrying ``format = "json"`` and the
   underlying ``serde_json::Error`` (``REQ_0213``).

   Decode delegates to ``serde_json::from_slice``; truncated /
   wrong-shape / wrong-type / empty input all surface as
   ``ConnectorError::Codec`` rather than being silently dropped
   (``REQ_0214``).

   **Tests.** TEST_0110 (round-trip proptest), TEST_0111 (encode
   error paths — see the amended :need:`TEST_0111` text routing
   buffer-too-small to ``PayloadOverflow``), TEST_0112 (decode
   error paths).

.. impl:: taktora-connector-host crate
   :id: IMPL_0040
   :status: open
   :implements: BB_0005
   :refines: REQ_0220, REQ_0223, REQ_0231, REQ_0270, REQ_0271, REQ_0272, REQ_0273, REQ_0847

   **Crate.** ``crates/taktora-connector-host``. Depends on
   ``taktora-connector-core``, ``taktora-connector-transport-iox``,
   ``taktora-executor``, ``crossbeam-channel``. Optional
   ``taktora-executor-tracing`` dep behind a default-off ``tracing``
   feature (``REQ_0273``).

   **Surface.**

   * ``Connector`` trait — associated ``Routing`` / ``Codec``
     types; methods ``name`` / ``health`` / ``subscribe_health``
     / ``register_with`` / ``create_writer<T, N>`` /
     ``create_reader<T, N>`` (``REQ_0220``, ``REQ_0223``). Not
     dyn-compatible — concrete connectors plug into the host one
     at a time via ``ConnectorHost::register<C: Connector>``.
   * ``HealthSubscription`` — receive-only handle wrapping a
     ``crossbeam_channel::Receiver<HealthEvent>``. Per the
     amended :need:`REQ_0231`, this is the in-process
     implementation of the spec's "observable handle" contract;
     the alternative cross-process form using
     ``taktora_executor::Channel<HealthEventWire>`` is deferred
     until a real connector exercises out-of-process health
     observation.
   * ``ConnectorHost::builder()`` + ``register`` +
     ``run`` / ``run_for`` / ``run_n`` (``REQ_0270``,
     ``REQ_0272``). Owns the underlying
     ``taktora_executor::Executor`` and exposes a ``Stoppable``
     handle for external shutdown.
   * ``ConnectorGateway::builder()`` — parallel construction for
     the gateway side (``REQ_0271``).

   **Deviation from :need:`REQ_0273`.** The default-off
   ``tracing`` cargo feature is wired (deps pull
   ``taktora-executor-tracing`` when the feature is on); the
   ``Observer`` adapter implementation that forwards
   ``HealthEvent`` and ``ExecutionMonitor`` callbacks through the
   global ``tracing`` subscriber is deferred until a real
   connector emits HealthEvents on a tracing subscriber under
   load. Tracked for a follow-on implementation commit.

   **Tests.** Integration test using a minimal in-tree
   ``EchoConnector`` exercises the full host
   register → run → executable-item-driven loop and confirms
   ``HealthSubscription`` delivers events published on the
   connector's internal health channel.

.. impl:: taktora-connector-ethercat crate (C5a–C5e + C7a)
   :id: IMPL_0050
   :status: open
   :implements: BB_0030
   :refines: REQ_0310, REQ_0311, REQ_0312, REQ_0313, REQ_0314, REQ_0315, REQ_0316, REQ_0317, REQ_0318, REQ_0319, REQ_0320, REQ_0321, REQ_0322, REQ_0323, REQ_0324, REQ_0325, REQ_0326, REQ_0327, REQ_0328, REQ_0329, REQ_0330, REQ_0331, REQ_0332, REQ_0333, REQ_0841, REQ_0842, REQ_0846, REQ_0847

   **Crate.** ``crates/taktora-connector-ethercat``. Default deps:
   ``taktora-connector-core``, ``taktora-connector-transport-iox``,
   ``taktora-connector-host``, ``taktora-executor``,
   ``crossbeam-channel``, ``tokio`` (``rt`` +
   ``rt-multi-thread`` + ``macros`` + ``sync``). Optional
   ``ethercrab`` dep behind the default-off ``bus-integration``
   cargo feature.

   **Status.** C5a + C5b land the protocol-agnostic core:
   routing, options builder, bridges, health monitor, tokio
   runtime lifecycle, ``Connector`` trait impl, and the
   pure-logic helpers (``sdo`` / ``scheduler`` / ``wkc``) that
   carry the gateway's load-bearing decision logic. C5c pulls
   ``ethercrab`` 0.7 as an optional dep and ships the
   forward-compatible declarations (``bus::EthercatPduStorage``
   type alias + ``declare_pdu_storage!`` macro) every
   application that wants real-bus deployment needs to declare
   anyway. The cycle-loop wiring against
   ``ethercrab::MainDevice`` was scoped to C5c but pulled back
   when ``ethercrab`` 0.7's actual API surface diverged from the
   examples reachable via documentation search; writing 1000+
   lines of speculative integration code against an API the
   author can't iterate against would have produced code that
   compiles but whose runtime behaviour is unverified — exactly
   the trust-me-but-untested posture the framework otherwise
   avoids.

   C5d takes the second path: defines the ``BusDriver`` trait
   that abstracts over "the operations the cycle loop needs from a
   real EtherCAT bus", ships an in-tree ``MockBusDriver`` that
   makes the cycle loop exhaustively testable without hardware,
   and a ``CycleRunner`` that composes ``CycleScheduler``,
   ``BusDriver``, ``evaluate_wkc``, and ``EthercatHealthMonitor``
   into one cycle-driving unit.

   C5e lands ``EthercrabBusDriver`` — a concrete ``BusDriver``
   wrapping ``ethercrab::MainDevice`` against ethercrab 0.7's
   API. The integration is **compile-checked only**: no EtherCAT
   hardware is available at the time of authoring, so runtime
   behaviour is unverified. The hardware-gated integration test
   under ``tests/ethercrab_driver.rs`` (``#[ignore]``-marked,
   gated on ``ETHERCAT_TEST_NIC``) documents the bring-up + cycle
   pattern and is one ``--ignored`` flag away from running on a
   Linux gateway host with ``CAP_NET_RAW``. End-to-end
   verification waits on hardware arrival and a follow-on commit
   to capture any API mismatches surfaced by the first real-bus
   run.

   **Surface.**

   * ``EthercatRouting`` — typed routing identifying one
     process-data slice by SubDevice address, PDO direction, bit
     offset, bit length. Implements ``Routing`` (``REQ_0311``).
   * ``EthercatConnectorOptions`` typed builder —
     ``cycle_time`` (default 2 ms, min 1 ms clamp; ``REQ_0316``),
     ``distributed_clocks`` opt-in (``REQ_0318``), bounded
     bridge capacities (``REQ_0322``), network interface name,
     ``&'static [SubDeviceMap]`` PDO descriptor (``REQ_0314``,
     :need:`ADR_0027`), tokio worker-thread count
     (:need:`ADR_0026`).
   * ``OutboundBridge<T>`` — bounded; saturation surfaces as
     ``OutboundError::BackPressure(T)`` (``REQ_0323``).
   * ``InboundBridge<T>`` — bounded; saturation drops the
     message and bumps a running count so the gateway can emit
     ``HealthEvent::DroppedInbound { count }`` (``REQ_0324``).
   * ``EthercatHealthMonitor`` — thread-safe wrapper around
     ``HealthMonitor`` that broadcasts every legal transition
     over a ``crossbeam_channel``.
   * ``EthercatGateway`` — owns its tokio runtime
     (multi-thread, default 1 worker per :need:`ADR_0026`) and
     joins it on ``Drop`` with a 5-second budget mirroring
     :need:`ARCH_0013` (``REQ_0321``).
   * ``EthercatConnector<D: BusDriver, C: PayloadCodec>`` —
     implements the framework ``Connector`` trait (``REQ_0310``).
     ``create_writer`` / ``create_reader`` open the plugin-side
     iceoryx2 service named ``"{descriptor.name()}.out"`` /
     ``".in"``, open the paired gateway-side raw port on the same
     service, and register the channel in the shared
     ``ChannelRegistry`` (``REQ_0223`` + ``REQ_0328``).
     ``register_with`` (C7b) takes the configured driver out of the
     connector and spawns ``dispatcher_loop`` on the gateway's
     tokio runtime (``REQ_0321``); the framework still receives a
     heartbeat ``ExecutableItem`` for ``REQ_0272``.
   * ``sdo::pdo_sdo_writes`` — pure function producing the
     ordered SDO write sequence (clear → entries → set-count
     on indices ``0x1C12`` and ``0x1C13``) that the gateway
     applies during the PRE-OP → SAFE-OP transition
     (``REQ_0315``).
   * ``scheduler::CycleScheduler`` — pure-clock pacing decision
     with skip-not-catch-up semantics; 10-cycle clock jump
     produces exactly one fire (``REQ_0317``).
   * ``wkc::evaluate_wkc`` + ``WkcVerdict::degraded_reason`` —
     working-counter health policy (``REQ_0319``, ``REQ_0320``).
   * ``driver::BusDriver`` — async trait abstracting the
     bring-up + per-cycle operations the runner needs from a
     concrete back-end (``REQ_0312`` / ``REQ_0313`` / ``REQ_0315``
     are encoded in the contract; concrete impls cover them).
     C7a extends the trait with callback-shaped
     ``with_subdevice_outputs_mut`` / ``with_subdevice_inputs``
     methods that expose one SubDevice's PDI region; the
     callback shape keeps ethercrab's internal ``PdiWriteGuard``
     lifetime scoped to the impl (``REQ_0326``, ``REQ_0327``).
   * ``mock::MockBusDriver`` — programmable test fixture: WKC
     sequences, configurable bring-up response, bring-up
     failure injection. C7a extends with per-SubDevice
     PDI buffers (``with_subdevice_outputs`` /
     ``with_subdevice_inputs`` builders + ``Mutex``-backed
     interior storage for the callback methods).
   * ``runner::CycleRunner<D: BusDriver>`` — composes
     ``CycleScheduler`` + ``BusDriver`` + ``evaluate_wkc`` +
     ``EthercatHealthMonitor``. End-to-end tested via
     ``MockBusDriver``.
   * ``pdi::write_routing`` / ``pdi::read_routing`` — pure-logic
     bit-slice translation between a per-SubDevice PDI buffer
     and a codec-encoded byte payload, honouring REQ_0311's
     ``bit_offset`` / ``bit_length``. Read-modify-write on
     partial leading / trailing bytes preserves adjacent
     slices (``REQ_0326``, ``REQ_0327``).
   * ``registry::ChannelRegistry`` — Vec-backed registry of
     ``RegisteredChannel { descriptor_name, routing,
     direction, binding }``. C7b extends ``ChannelBinding`` with
     ``Outbound(Box<dyn OutboundDrain>)`` and
     ``Inbound(Box<dyn InboundPublish>)`` variants carrying the
     gateway-side iceoryx2 ports (trait objects erase the
     channel's user-type ``T`` and codec ``C``).
     Insertion-order iteration verified by TEST_0219; per-cycle
     ``iter()`` is allocation-free (verified via
     ``CountingAllocator`` across 1 000 cycles × 16 channels —
     ``REQ_0328``).
   * ``dispatcher::dispatch_one_cycle`` /
     ``dispatcher::dispatcher_loop`` (C7b) — gateway-side
     byte-shovel composing ``pdi::write_routing`` /
     ``pdi::read_routing`` + ``ChannelRegistry`` + the iceoryx2
     raw pub/sub ports. ``dispatch_one_cycle`` is the
     single-iteration synchronous form used by the
     ``TEST_0220`` / ``TEST_0221`` / ``TEST_0222`` integration
     tests; ``dispatcher_loop`` is the long-running ``async fn``
     spawned by ``register_with``. The trait-object wrappers
     ``IoxOutboundDrain<N>`` / ``IoxInboundPublish<N>`` adapt the
     raw iceoryx2 reader / writer to ``OutboundDrain`` /
     ``InboundPublish`` (``REQ_0326``, ``REQ_0327``, ``REQ_0328``).
   * ``raw::RawChannelWriter<N>`` / ``raw::RawChannelReader<N>``
     in ``taktora-connector-transport-iox`` — byte-only iceoryx2
     ports used by the dispatcher. ``send_raw_bytes`` /
     ``try_recv_into`` bypass the codec entirely, keeping the
     dispatcher hot path codec-free (``REQ_0327`` amended in
     C7b).

   **Verification posture.** Every REQ covered by IMPL_0050 has
   a passing unit / integration test on every CI push.
   ``EthercrabBusDriver`` provides the real-bus path for
   REQ_0312 (single MainDevice — one ``PduStorage::try_split``
   per driver), REQ_0313 (bus reaches OP — ``group.into_op``
   fast path), REQ_0314 + REQ_0315 (PDO mapping applied via
   ``pdo_sdo_writes`` + ``sdo_write`` during PRE-OP), and
   REQ_0325 (Linux raw socket — ``tx_rx_task``); those tests
   await physical hardware under ``ETHERCAT_TEST_NIC``.
   ``REQ_0326`` / ``REQ_0327`` / ``REQ_0328``'s end-to-end byte
   hops are exercised against ``MockBusDriver`` via the C7b
   integration tests ``TEST_0220`` (outbound),
   ``TEST_0221`` (inbound), and ``TEST_0222`` (loopback
   round-trip), so the iceoryx2 ↔ PDI ↔ iceoryx2 pipeline is
   green in every CI run without hardware.

   **Tests.** Cases pass: TEST_0201 (routing round-trip),
   TEST_0204 + TEST_0206 (options builder), TEST_0205-partial
   (SDO write sequence shape), TEST_0207 (cycle scheduler
   skip-not-catch-up), TEST_0208 (DC opt-in flag), TEST_0209 +
   TEST_0210 (WKC policy), TEST_0211-partial (gateway tokio
   runtime ownership and clean drop), TEST_0212-0214 (bridge
   bounded capacity, BackPressure, DroppedInbound), TEST_0216-
   0218 (PDI bit-slice byte-aligned / unaligned round-trips,
   adjacent-slice preservation), TEST_0219 (registry
   alloc-free iter), TEST_0220 (outbound end-to-end), TEST_0221
   (inbound end-to-end), TEST_0222 (loopback round-trip via
   mock), plus surface-shape checks for TEST_0200 (Connector
   trait surface, ``create_writer`` / ``create_reader``
   registration semantics).

.. impl:: taktora-connector-zenoh crate (planned)
   :id: IMPL_0060
   :status: draft
   :implements: BB_0040
   :refines: REQ_0400, REQ_0401, REQ_0402, REQ_0403, REQ_0404, REQ_0405, REQ_0406, REQ_0407, REQ_0408, REQ_0420, REQ_0421, REQ_0422, REQ_0423, REQ_0424, REQ_0425, REQ_0426, REQ_0427, REQ_0428, REQ_0440, REQ_0442, REQ_0443, REQ_0444, REQ_0445, REQ_0446, REQ_0847

   **Crate.** ``crates/taktora-connector-zenoh`` (planned; not yet
   scaffolded). Default deps: ``taktora-connector-core``,
   ``taktora-connector-transport-iox``, ``taktora-connector-host``,
   ``taktora-executor``, ``crossbeam-channel``, ``tokio`` (``rt`` +
   ``rt-multi-thread`` + ``macros`` + ``sync``). Optional
   ``zenoh`` dep behind the default-off ``zenoh-integration``
   cargo feature (:need:`REQ_0444`).

   **Status.** Planned surface only — the crate has not been
   scaffolded. This directive locks in the public API that the
   forthcoming implementation will be measured against. Once the
   crate exists, its surface, dependencies, and test coverage are
   reconciled against this directive; divergences are recorded as
   amendments. Status flips from ``draft`` to ``open`` after the
   first complete in-tree implementation lands and the public
   surface matches the bulleted list below.

   **Surface.**

   * ``ZenohRouting`` — typed routing carrying the Zenoh key
     expression and pub/sub QoS knobs (``key_expr``,
     ``congestion_control``, ``priority``, ``reliability``,
     ``express``). Implements ``Routing``. ``key_expr`` is
     validated on the plugin side at ``create_writer`` /
     ``create_reader`` / ``create_querier`` /
     ``create_queryable`` entry; invalid expressions surface as
     ``ConnectorError::Configuration`` (``REQ_0401``,
     :need:`ADR_0042`).
   * ``ZenohConnectorOptions`` typed builder — ``mode``
     (``SessionMode::{Peer, Client, Router}``, default ``Peer``;
     ``REQ_0440``), ``connect`` / ``listen`` locator lists
     surfaced verbatim to ``zenoh::Config`` (``REQ_0443``),
     ``query_target`` / ``query_consolidation`` / ``query_timeout``
     defaults for queriers (``REQ_0425``), bounded bridge
     capacities (``REQ_0404``), optional ``min_peers`` knob
     governing ``Degraded`` health transitions.
   * ``OutboundBridge<T>`` — bounded; saturation surfaces as
     ``ConnectorError::BackPressure`` and flips health to
     ``Degraded`` (``REQ_0405``).
   * ``InboundBridge<T>`` — bounded; saturation drops the offending
     Zenoh sample or reply chunk and bumps a running count so the
     gateway emits ``HealthEvent::DroppedInbound { count }``
     (``REQ_0406``, ``REQ_0428``).
   * ``ZenohSessionLike`` — trait abstracting over ``zenoh::Session``
     vs ``MockZenohSession`` so the gateway is compile-time
     monomorphised against either back-end. Methods:
     ``declare_publisher`` / ``declare_subscriber`` / ``get`` /
     ``declare_queryable`` / ``session_state``.
   * ``MockZenohSession`` — in-process pub/sub + query loopback
     implementing ``ZenohSessionLike``; ships in the default
     build, not gated by ``zenoh-integration`` (``REQ_0445``).
     Programmable session-state sequences for health-state tests.
   * ``RealZenohSession`` — thin wrapper over ``zenoh::Session``
     created via ``zenoh::open(config)``; lives behind the
     ``zenoh-integration`` cargo feature.
   * ``ZenohGateway<S: ZenohSessionLike, C: PayloadCodec>`` —
     owns one session, the per-channel routing registry, the
     bounded bridges, the tokio runtime hosting Zenoh callbacks
     (``REQ_0403``), and the ``correlation_id → zenoh::Query``
     map for in-flight queryable reply streams (``REQ_0426``).
     Observes session-alive ↔ session-closed transitions from
     ``S`` and emits one ``HealthEvent`` per transition
     (``REQ_0442``). No ``ReconnectPolicy``
     (:need:`ADR_0041` / :need:`REQ_0441`).
   * ``ZenohConnector<C: PayloadCodec>`` — implements the
     framework ``Connector`` trait with ``type Routing =
     ZenohRouting`` and ``type Codec = C`` (``REQ_0400``).
     ``create_writer`` / ``create_reader`` open the plugin-side
     iceoryx2 services named ``"{descriptor.name()}.out"`` /
     ``".in"`` (``REQ_0407``, ``REQ_0408``). Beyond the trait,
     ``ZenohConnector`` exposes two **concrete** non-trait
     methods — ``create_querier<Q, R, …>`` and
     ``create_queryable<Q, R, …>`` — returning Zenoh-specific
     handle types (``REQ_0420``, :need:`ADR_0040`).
   * ``ZenohQuerier<Q, R, C, N>`` — non-trait query-initiation
     handle. ``send(q)`` mints a fresh ``QueryId``, encodes ``Q``
     via the connector's codec, stamps the ``QueryId`` into the
     envelope's ``correlation_id`` (``REQ_0421``), and publishes
     on ``"{name}.query.out"``; returns the ``QueryId`` for
     reply demultiplexing. ``try_recv`` drains
     ``"{name}.reply.in"``, decoding the 1-byte frame
     discriminator (``0x01`` data, ``0x02`` end-of-stream,
     ``0x03`` gateway-synthetic timeout per :need:`ADR_0043` /
     ``REQ_0424``). Per-call ``send_with_timeout`` overrides
     the session-wide default (``REQ_0425``).
   * ``ZenohQueryable<Q, R, C, N>`` — non-trait query-handling
     handle. ``try_recv`` surfaces ``(QueryId, Q)`` decoded from
     ``"{name}.query.in"``. ``reply(id, r)`` encodes ``R``
     into ``envelope.payload[1..]`` with byte ``[0] = 0x01``
     and publishes on ``"{name}.reply.out}"``; callable zero
     or more times per ``QueryId`` (``REQ_0423``, ``REQ_0427``).
     ``terminate(id)`` publishes a ``0x02`` envelope; the
     gateway drops the corresponding ``zenoh::Query`` handle,
     finalising the upstream reply stream (``REQ_0426``).
     Matching of ``QueryId`` to ``zenoh::Query`` lives inside
     this type and the gateway — never the framework
     (preserves :need:`REQ_0290`, ``REQ_0422``).
   * Reply framing — every envelope on ``"{name}.reply.out"`` /
     ``"{name}.reply.in"`` carries a 1-byte Zenoh-private
     discriminator at ``payload[0]``: ``0x01`` data chunk
     (followed by codec-encoded ``R``), ``0x02`` end of stream,
     ``0x03`` gateway-synthetic timeout terminator. The
     framework's ``ConnectorEnvelope`` reserved word stays
     untouched (``REQ_0424``, :need:`ADR_0043`).
   * Codec is a generic parameter, compile-time-monomorphised
     (re-affirms :need:`REQ_0211`); ``JsonCodec`` is the default
     wiring in examples (``REQ_0402``, re-affirms
     :need:`REQ_0212`). Gateway-side dispatching forwards raw
     bytes only — codecs run plugin-side on both pub/sub and
     query paths (``REQ_0408``, ``REQ_0427``).
   * Cross-platform — Linux, macOS, and Windows are supported
     host operating systems for both plugin and gateway
     (``REQ_0446``); no Linux-specific socket capability is
     required (contrast :need:`REQ_0325` for EtherCAT).

   **Tests.** The corpus authored alongside this directive in
   :doc:`../../verification/connector/index` includes TEST_0300
   (``ZenohRouting`` validation), TEST_0301 (``Connector`` trait
   surface), TEST_0302 (pub/sub end-to-end via
   ``MockZenohSession``), TEST_0303 (query round-trip via
   ``MockZenohSession``), TEST_0304 (codec failure paths),
   TEST_0305 / TEST_0306 (bridge saturation), TEST_0307
   (query-timeout terminator), TEST_0308 (health state machine),
   TEST_0309 (anti-req — no ``ReconnectPolicy``), TEST_0310
   (cargo-feature dep gating), TEST_0311 (cross-platform build
   matrix), TEST_0314 (tokio sidecar containment). Layer-2 /
   Layer-3 cases ``TEST_0312`` (two-peer real session) and
   ``TEST_0313`` (client-mode router smoke) remain
   ``:status: draft`` until the ``zenoh-integration`` and
   ``ZENOH_TEST_ROUTER`` CI jobs land.

.. impl:: taktora-connector-can crate (layer-1)
   :id: IMPL_0080
   :status: open
   :implements: BB_0070
   :refines: REQ_0600, REQ_0601, REQ_0602, REQ_0603, REQ_0604, REQ_0605, REQ_0606, REQ_0607, REQ_0608, REQ_0610, REQ_0611, REQ_0612, REQ_0613, REQ_0614, REQ_0615, REQ_0620, REQ_0621, REQ_0622, REQ_0623, REQ_0624, REQ_0625, REQ_0630, REQ_0631, REQ_0632, REQ_0633, REQ_0634, REQ_0635, REQ_0636, REQ_0847

   **Crate.** ``crates/taktora-connector-can`` (layer-1 landed;
   real ``socketcan::tokio::*`` integration deferred to layer-2).
   Default deps: ``taktora-connector-core``,
   ``taktora-connector-transport-iox``, ``taktora-connector-host``,
   ``taktora-executor``, ``crossbeam-channel``, ``tokio`` (``rt`` +
   ``rt-multi-thread`` + ``macros`` + ``sync``). Optional
   ``socketcan`` dep (with its ``tokio`` feature) behind the
   default-off ``socketcan-integration`` cargo feature
   (:need:`REQ_0603`).

   **Status.** Planned surface only — the crate has not been
   scaffolded. This directive locks in the public API that the
   forthcoming implementation will be measured against. Once the
   crate exists, its surface, dependencies, and test coverage are
   reconciled against this directive; divergences are recorded as
   amendments. Status flips from ``draft`` to ``open`` after the
   first complete in-tree implementation lands and the public
   surface matches the bulleted list below.

   **Surface.**

   * ``CanIface`` — bounded ASCII string newtype of
     ``IFNAMSIZ`` − 1 = 15 bytes, validated on construction;
     wraps the Linux network interface name (``can0``, ``vcan0``,
     etc.). Implements ``Copy``, ``Eq``, ``Hash``.
   * ``CanId { value: u32, extended: bool }`` — typed identifier
     newtype carrying both 11-bit (standard) and 29-bit
     (extended) CAN identifiers; the ``extended`` flag is
     preserved end-to-end (:need:`REQ_0615`). Constructors
     ``CanId::standard(u16)`` / ``CanId::extended(u32)`` enforce
     the per-form bit-width invariant.
   * ``CanFrameKind`` — enum ``{ Classical, Fd }``. Determines
     ``ChannelDescriptor::max_payload_size`` deterministically
     (8 / 64 bytes per :need:`REQ_0612`).
   * ``CanFdFlags`` — bitflags ``{ BRS, ESI }`` carried in
     ``CanRouting`` for FD channels; ignored when
     ``kind == Classical``.
   * ``CanRouting`` — typed routing carrying ``iface``, ``can_id``,
     ``mask: u32``, ``kind``, ``fd_flags``. Implements ``Routing``
     (:need:`REQ_0601`). Plugin-side validation runs inside
     ``create_writer`` / ``create_reader`` before any iceoryx2
     service is created — invalid iface or payload-kind mismatch
     yields ``ConnectorError::Configuration``.
   * ``CanConnectorOptions`` typed builder — ``ifaces: Vec<CanIface>``
     listing the gateway-owned interfaces (:need:`REQ_0620`),
     ``outbound_bridge_capacity`` / ``inbound_bridge_capacity``
     (:need:`REQ_0606`), ``reconnect_policy: Box<dyn ReconnectPolicy>``
     with ``ExponentialBackoff::default()`` (:need:`REQ_0634`),
     and a ``recovery_window: Duration`` controlling the
     error-passive → Up debounce on :need:`ARCH_0062`.
   * ``OutboundBridge<T>`` — bounded per-iface; saturation
     surfaces as ``ConnectorError::BackPressure`` and flips
     health to ``Degraded`` (:need:`REQ_0607`).
   * ``InboundBridge<T>`` — bounded per-iface; saturation drops
     the offending CAN frame and bumps a running count so the
     gateway emits ``HealthEvent::DroppedInbound { count }``
     (:need:`REQ_0608`).
   * ``CanInterfaceLike`` — trait abstracting over real
     SocketCAN sockets vs ``MockCanInterface`` so the gateway is
     compile-time monomorphised against either back-end.
     Methods: ``send_classical`` / ``send_fd`` / ``recv`` /
     ``apply_filter`` / ``state``. ``recv`` returns an enum
     mirroring the upstream ``socketcan::CanFrame`` discriminant
     (``Data | Remote | Error``) so error frames are surfaced on
     the same call as data frames and routed to the gateway's
     classifier without a separate code path.
   * ``MockCanInterface`` — in-process loopback implementing
     ``CanInterfaceLike``; ships in the default build, not gated
     by ``socketcan-integration`` (:need:`REQ_0604`). Programmable
     error-frame injection for testing
     ``Connecting → Up → Degraded → Down → Connecting`` walks
     against :need:`ARCH_0062`.
   * ``RealCanInterface`` — landed in layer-2. Wraps a single
     ``socketcan::tokio::CanFdSocket`` per interface; the kernel's
     FD-aware socket (set via ``CAN_RAW_FD_FRAMES``, applied
     automatically by ``CanFdSocket::open_addr``) transparently
     handles both classical and FD frames so one socket per
     interface suffices regardless of which frame kinds the
     registered channels declare. Lives behind the
     ``socketcan-integration`` cargo feature (which also enables
     the upstream crate's ``tokio`` feature) and is target-gated
     to ``cfg(target_os = "linux")`` because the ``socketcan``
     crate's build script rejects non-Linux targets
     (:need:`REQ_0502`). The dep itself is declared under a
     ``[target.'cfg(target_os = "linux")'.dependencies]`` table so
     enabling the feature on macOS / Windows is a no-op rather
     than a build failure. Owns the ``CAP_NET_RAW`` socket bind
     and calls ``set_error_filter_accept_all`` at open + on every
     reopen to enable error-frame reporting (:need:`REQ_0631`).
     ``recv`` translates ``CanAnyFrame`` (``Normal(Data/Remote/Error)``
     or ``Fd``) into the framework's ``CanFrame`` enum;
     ``CanError`` is classified via the table in
     :need:`ARCH_0062` (``BusOff``, ``ControllerProblem(Receive|
     Transmit Error{Warning|Passive})``, ``LostArbitration``, …).
     The Linux raw-socket smoke test (:need:`TEST_0512`) brings
     up ``vcan0`` via ``modprobe vcan`` and runs the ignored
     ``tests/vcan_smoke.rs`` integration test with
     ``--include-ignored`` so it only fires on the ``vcan-smoke``
     CI job, never on plain ``cargo test``.
   * ``PerIfaceFilter`` (pure-logic helper, :need:`BB_0074`) —
     compiles the union of ``(can_id, mask, extended)`` tuples
     from registered readers into a single
     ``Vec<libc::can_filter>`` (or the ``socketcan`` crate's
     equivalent) suitable for one ``setsockopt(SOL_CAN_RAW,
     CAN_RAW_FILTER, …)`` call. Symmetric counterpart for the
     demux side: ``matching_readers(&frame) → impl Iterator<Reader>``
     so every matching reader gets its own envelope copy
     (:need:`REQ_0624`).
   * ``CanGateway<I: CanInterfaceLike, C: PayloadCodec>`` —
     owns one ``I`` per configured iface, per-iface routing
     registries, the bounded bridges, the tokio runtime hosting
     RX/TX tasks (:need:`REQ_0605`), and the per-iface
     ``HealthSubState`` map aggregated via worst-of
     (:need:`REQ_0630`). Observes error frames and updates
     ``ConnectorHealth`` via :need:`ARCH_0062`; emits one
     ``HealthEvent`` per transition (:need:`REQ_0635`).
     ``ReconnectPolicy`` is owned at this layer
     (:need:`REQ_0634`).
   * ``CanConnector<C: PayloadCodec>`` — implements the framework
     ``Connector`` trait with ``type Routing = CanRouting`` and
     ``type Codec = C`` (:need:`REQ_0600`). ``create_writer`` /
     ``create_reader`` open the plugin-side iceoryx2 services
     named ``"{descriptor.name()}.out"`` / ``".in"`` and, on the
     gateway side, trigger a per-iface filter recompute
     (:need:`REQ_0623`).
   * Linux-only real I/O — the gateway requires Linux ``PF_CAN``
     and the ``CAP_NET_RAW`` capability (:need:`REQ_0602`,
     mirrors :need:`REQ_0325`). The plugin-side ``CanConnector``
     and ``MockCanInterface`` stay portable to macOS and Windows
     for layer-1 development; the ``socketcan-integration``
     feature is the Linux gate.
   * Gateway-side dispatching forwards raw bytes only — codecs
     run plugin-side on both send and receive paths
     (:need:`REQ_0614`, mirrors :need:`REQ_0408` /
     :need:`REQ_0327`).

   **Tests.** The corpus authored alongside this directive in
   :doc:`../../verification/connector/index` includes TEST_0500
   (``CanConnector`` trait surface), TEST_0501 (``CanRouting``
   field round-trip), TEST_0502 / TEST_0503 (classical and FD
   round-trip via ``MockCanInterface``), TEST_0504 (per-iface
   filter union), TEST_0505 (multi-iface inbound demux),
   TEST_0506 (bus-off → Down → ReconnectPolicy reopen),
   TEST_0507 (error-passive → Degraded → recovery), TEST_0508
   (tokio sidecar containment), TEST_0509 / TEST_0510 (bridge
   saturation), TEST_0511 (cargo-feature dep gating), TEST_0513
   (anti-req — error frames not on plugin channel), TEST_0514
   (per-iface registry alloc-free iteration). Layer-2
   ``TEST_0512`` (Linux raw-socket smoke against ``vcan0``)
   remains ``:status: draft`` until the
   ``socketcan-integration`` CI job and the kernel ``vcan``
   module are wired into CI.

.. impl:: taktora-connector-j1939 crate (planned)
   :id: IMPL_0090
   :status: draft
   :implements: BB_0098
   :refines: REQ_0890, REQ_0891, REQ_0892, REQ_0893, REQ_0894, REQ_0895, REQ_0896, REQ_0897, REQ_0898, REQ_0899

   **Crate.** ``crates/taktora-connector-j1939`` (planned; not yet
   scaffolded). Default deps: ``taktora-connector-core``,
   ``taktora-connector-transport-iox`` (envelope **and** the
   :need:`BB_0097` slice channel), ``taktora-connector-host``,
   ``taktora-connector-codec``, ``taktora-executor``, ``tokio``, and
   ``taktora-connector-can`` for its ``CanInterfaceLike`` driver layer.
   Real CAN I/O is reached through ``taktora-connector-can``'s existing
   ``socketcan-integration`` feature; ``MockJ1939Interface`` ships
   unfeature-gated for layer-1 tests (:need:`REQ_0899`). Paired with a
   ``publish = false`` ``taktora-connector-j1939-tests`` crate per the
   connector guide's two-crate split.

   **Status.** Planned surface only — the crate has not been scaffolded.
   This directive locks the public API the forthcoming implementation is
   measured against; status flips ``draft`` → ``open`` once the crate
   lands and its surface matches the bulleted list below. The boundary,
   delivery, and address-claim decisions are :need:`ADR_0108`,
   :need:`ADR_0109`, and :need:`ADR_0110`.

   **Surface.**

   * ``J1939Routing`` — typed routing carrying ``pgn``, optional
     ``source_addr`` / ``dest_addr`` filters, transport class
     (``SingleFrame`` | ``Tp { max_len }``), and TX ``priority``.
     Implements ``Routing``; PDU1/PDU2 derived from the PGN's PF.
     Channel ``N`` validated against the transport class at
     ``create_writer`` / ``create_reader`` (``REQ_0891``).
   * ``J1939Connector<C: PayloadCodec>`` implementing ``Connector``
     with ``type Routing = J1939Routing`` (:need:`BB_0099`).
   * ``J1939ConnectorOptions`` typed builder — per-interface source
     address + 64-bit ``Name``, ``max_etp_bytes``, max-concurrent-TP-
     sessions, TP timer overrides, and bounded bridge capacities;
     multi-interface like ``CanConnectorOptions``.
   * ``MockJ1939Interface`` over ``MockCanInterface`` (:need:`BB_0103`).

   **Tests.** TEST_0886 (ID decode + PGN routing), TEST_0887 (N
   validation), TEST_0888 (BAM), TEST_0889 (RTS/CTS), TEST_0890 (ETP +
   bounded abort), TEST_0891 (TP timeout → health), TEST_0892 (session
   bound), TEST_0893 (claim contention), TEST_0894 (claim → health + TX
   gate), TEST_0895 (mock harness).

.. impl:: taktora-connector-transport-iox slice channel (planned)
   :id: IMPL_0091
   :status: draft
   :implements: BB_0097
   :refines: REQ_0885, REQ_0886, REQ_0887, REQ_0888, REQ_0889

   **Crate.** ``crates/taktora-connector-transport-iox`` (additive
   surface; the crate already ships ``ConnectorEnvelope<N>`` per
   :need:`IMPL_0020`). Adds ``SliceChannelWriter`` / ``SliceChannelReader``
   over an iceoryx2 slice (``[u8]``) publish-subscribe service with an
   iceoryx2 user-header carrying ``sequence_number`` / ``timestamp_ns``.

   **Status.** Planned surface only. Status flips ``draft`` → ``open``
   once the slice channel lands and the ETP path (:need:`BB_0101`)
   consumes it.

   **Surface.**

   * ``SliceChannelWriter`` — ``send(&[u8])`` loaning a runtime-sized
     sample via ``loan_slice_uninit`` (``REQ_0885``, ``REQ_0886``);
     ``initial_max_slice_len`` + ``AllocationStrategy::PowerOfTwo``
     growth bounded by ``max_payload_bytes`` (``REQ_0887``,
     ``REQ_0888``).
   * ``SliceChannelReader`` — yields one variable-length message per
     sample, exposing the user-header metadata (``REQ_0889``).

   **Tests.** TEST_0884 (variable-length round-trip), TEST_0885 (growth
   + ceiling enforcement).
