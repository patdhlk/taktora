.. _connector-arc42-solution-strategy:

Solution strategy
=================

arc42 §4.

The framework's shape is the consequence of ten architectural decisions
made during brainstorming. Each decision is captured here as an ADR
that ``:refines:`` the requirement or feature it answers.

.. arch-decision:: Spec scope — framework core + MQTT reference
   :id: ADR_0001
   :status: open
   :refines: FEAT_0030

   **Context.** Four protocol connectors (MQTT, OPC UA, gRPC, ADS) and
   three codecs (JSON, Protobuf, MessagePack) were on the table. Each
   protocol introduces its own design quirks; specifying all four in
   one round risks the spec drifting into protocol-specific minutiae.

   **Decision.** This spec covers the framework core plus **MQTT** as
   the reference connector. OPC UA / gRPC / ADS get follow-on specs
   reusing the same five contracts.

   **Consequences.** ✅ Spec stays focused on the framework's contracts.
   ✅ MQTT exercises every contract (codec, routing, health, reconnect)
   end-to-end. ❌ Other connector specs are blocked on this one
   landing.

.. arch-decision:: Umbrella feature is a peer of FEAT_0010
   :id: ADR_0002
   :status: open
   :refines: FEAT_0030

   **Context.** :need:`FEAT_0010` "PLC runtime heart" is the existing
   top-level umbrella, with :need:`FEAT_0023` "Fieldbus integration
   interface" as a sub-feature. The connector framework is broader than
   fieldbus (MQTT and gRPC are application-protocol level).

   **Decision.** Add :need:`FEAT_0030` "Connector framework" as a peer
   top-level feature, not under :need:`FEAT_0010`. :need:`FEAT_0023`
   later ``:refines:`` :need:`FEAT_0030` when an ADS connector spec
   lands.

   **Consequences.** ✅ Honest semantics — the framework is general
   purpose, not PLC-bound. ❌ The spec now has two top-level umbrellas,
   which the overview page should explicitly explain.

.. arch-decision:: Both deployment shapes supported
   :id: ADR_0003
   :status: open
   :refines: FEAT_0035

   **Context.** Gateway-as-separate-process gives fault isolation
   (:need:`QG_0001`); gateway-as-tokio-task is operationally simpler
   (one binary, one signal handler). Different consumers want different
   trade-offs.

   **Decision.** Define the framework so the same envelope/iceoryx2
   contract works in either deployment. The host wires the gateway as
   a tokio task or a separate binary using identical code; only
   process-startup differs.

   **Consequences.** ✅ Fault-isolation-conscious deployments and
   single-binary deployments share one framework. ❌ Both paths must be
   tested; shutdown coordination is specified twice (in-process,
   out-of-process), but the SHM mechanics are unchanged.

.. arch-decision:: Per-channel envelope size, declared in descriptor
   :id: ADR_0004
   :status: open
   :refines: REQ_0201

   **Context.** A universal 64 KB envelope (the C# Apex.Ida pattern)
   wastes shared memory for small messages and refuses large ones.
   iceoryx2's typed services support per-service payload sizes.

   **Decision.** ``ChannelDescriptor`` carries a per-channel max
   payload size (via const generic ``N``); each iceoryx2 service is
   typed on ``ConnectorEnvelope<N>`` for its compile-time-chosen ``N``.

   **Consequences.** ✅ Memory sized to the workload. ✅ Type system
   prevents publishers and subscribers from disagreeing on size. ❌
   Different channels are different types; const-generic
   monomorphisation could grow code size if many channel sizes are
   used (see :need:`RISK_0003`).

.. arch-decision:: Codec is a generic parameter on the connector
   :id: ADR_0005
   :status: open
   :refines: REQ_0211

   **Context.** Two clean alternatives existed: type-erased
   ``Box<dyn PayloadCodec>`` (runtime-swappable, ``erased_serde``
   indirection) or generic-on-connector (``MqttConnector<C>``,
   compile-time-monomorphised).

   **Decision.** Generic-on-connector. Concrete connector types are
   ``MqttConnector<JsonCodec>``, ``MqttConnector<MsgPackCodec>``, etc.

   **Consequences.** ✅ Zero dynamic dispatch on the hot path. ✅ Codec
   errors carry a static ``format_name``. ❌ Cannot swap codec at
   runtime; code must rebuild to change codec for a connector.

.. arch-decision:: Explicit-builder plugin discovery
   :id: ADR_0006
   :status: open
   :refines: REQ_0270

   **Context.** Two alternatives: ``inventory``-crate compile-time
   registration (link-time globals collect ``ConnectorRegistration``
   entries) versus an explicit builder (``ConnectorHost::builder()
   .with(MqttConnector::<JsonCodec>::new(...)).build()``).

   **Decision.** Explicit builder. Matches taktora-executor's existing
   ``Executor::builder()`` idiom.

   **Consequences.** ✅ One file you can grep for the wiring; no
   link-time global state alongside the compile-time generics. ❌
   Adding a connector requires rebuilding the host (already true given
   :need:`ADR_0005`).

.. arch-decision:: Plugin and gateway are both taktora-executor consumers
   :id: ADR_0007
   :status: open
   :refines: CON_0001

   **Context.** Three options: tokio-only gateway (separate world from
   plugin), taktora-executor on both sides with tokio bridged in, or
   raw-iceoryx2 gateway emitting unified observability.

   **Decision.** Both halves are ``ExecutableItem``-based. Tokio runs
   as a sidecar inside connector crates; taktora-executor's
   ``Channel<T>`` bridges the two. One programming model, one
   observability surface, one shutdown story.

   **Consequences.** ✅ ``Observer`` and ``ExecutionMonitor`` cover the
   gateway for free. ✅ SIGINT-clean-exit story propagates without
   extra plumbing. ❌ The bridge is the place latency can be
   introduced; bridge-channel sizing matters.

.. arch-decision:: Routing carried as a typed struct
   :id: ADR_0008
   :status: open
   :refines: REQ_0221

   **Context.** Three positions: opaque channel name + side-channel
   YAML config; channel name + typed routing struct; channel name +
   key-value attribute bag.

   **Decision.** Typed routing struct (``MqttRouting``, ``OpcUaRouting``,
   ...) implementing the ``Routing`` marker trait, embedded in
   ``ChannelDescriptor``.

   **Consequences.** ✅ Routing is part of the public, type-checked API.
   ✅ Catches misspelled / missing fields at compile time. ❌ Plugin
   code is connector-aware (no protocol-portable channels — see
   :need:`REQ_0294`).

.. arch-decision:: Lifecycle = ReconnectPolicy + ConnectorHealth
   :id: ADR_0009
   :status: open
   :refines: FEAT_0034

   **Context.** Different protocol stacks own reconnect differently —
   ``rumqttc`` exposes raw connect events (fits a policy trait);
   ``tonic`` manages reconnect inside the channel (no hooks); OPC UA
   sessions sit in between.

   **Decision.** Provide both a ``ReconnectPolicy`` trait + default
   ``ExponentialBackoff`` (used by stacks that surface raw events) AND
   a ``ConnectorHealth`` state machine emitted via ``HealthEvent``
   (uniform observability regardless of who owns reconnect).

   **Consequences.** ✅ Stacks that fit a uniform policy aren't
   reinventing backoff; stacks that handle reconnect internally aren't
   forced into a foreign mechanism. ❌ Two ways to get reconnect
   means new connector authors must pick the right one for their
   protocol.

.. arch-decision:: MQTT scope — realistic but bounded
   :id: ADR_0010
   :status: open
   :refines: FEAT_0036

   **Context.** "Reference connector" must exercise enough of the
   framework's contracts to validate them, without ballooning into
   MQTT-protocol-minutiae territory.

   **Decision.** Pub+sub, QoS 0+1, retained messages, wildcard
   subscriptions, username/password auth, optional TLS, MQTT 3.1.1.
   Defer: QoS 2, MQTT 5, LWT, persistent sessions, client-cert TLS.

   **Consequences.** ✅ Each deferred feature exercises framework
   contracts — adding them later doesn't reshape the framework.
   ❌ MQTT 5 user-properties / shared-subscriptions adoption is
   blocked on a follow-on spec.

.. arch-decision:: ethercrab as the EtherCAT MainDevice library
   :id: ADR_0020
   :status: open
   :refines: FEAT_0041

   **Context.** EtherCAT MainDevice options in Rust are ``ethercrab``
   (pure Rust, ``std`` + ``no_std``, actively maintained), ``soem-rs``
   (FFI wrapper around the C SOEM stack), or hand-rolled. SOEM is the
   industry-standard C implementation, but pulling C dependencies and
   their build complexity into the workspace conflicts with the
   no-C-deps posture the rest of taktora adopts.

   **Decision.** Use ``ethercrab`` from the workspace. It is pure
   Rust, supports both ``std`` (tokio TX/RX task on Linux raw socket)
   and ``no_std`` (deferred), and exposes a typestate bring-up API
   (``init_single_group`` → ``into_op``) that maps cleanly onto the
   four EtherCAT bus states.

   **Consequences.** ✅ No C build dependencies; one ``cargo build``
   gets everything. ✅ ``no_std`` deployment becomes possible without
   a second EtherCAT stack. ❌ ethercrab is pre-1.0, so API churn is
   a tracked risk. ❌ SOEM conformance test coverage is broader;
   ethercrab is validated against EK1100 / EL-series modules but
   uncommon vendor extensions may surface gaps.

.. arch-decision:: Single MainDevice per gateway
   :id: ADR_0021
   :status: open
   :refines: REQ_0312

   **Context.** An EtherCAT network is physically one segment per
   network interface; the MainDevice owns that segment's TX/RX cycle.
   Multi-NIC support would require multiple MainDevices arbitrating
   shared cycle timing and working-counter state.

   **Decision.** Each ``EthercatGateway`` instance owns exactly one
   ``ethercrab::MainDevice`` bound to one network interface. Multi-NIC
   deployments instantiate multiple gateways with disjoint SHM service
   names.

   **Consequences.** ✅ Cycle timing, working-counter ownership, and
   Distributed Clocks bring-up have a single source of truth.
   ✅ Mirrors :need:`REQ_0295` (one broker per MQTT gateway).
   ❌ Operators wanting one process to own two EtherCAT segments must
   instantiate two gateways (acceptable — rare configuration).

.. arch-decision:: Static PDO mapping declared at build time
   :id: ADR_0022
   :status: open
   :refines: REQ_0314, REQ_0315

   **Context.** EtherCAT SubDevice PDO mappings can be sourced two
   ways: (1) parsing an ESI / EEPROM XML descriptor per SubDevice at
   startup, or (2) declaring the mapping in application code at build
   time. ESI parsing is what TwinCAT and similar engineering tools
   do; it handles arbitrary vendor modules. Static declaration trades
   generality for compile-time type safety on the routing struct.

   **Decision.** The application declares each SubDevice's PDO
   mapping as a static description in ``EthercatConnectorOptions``;
   the gateway applies it during the PRE-OP → SAFE-OP transition via
   SDO writes to the sync-manager assignment indices ``0x1C12``
   (RxPDO) and ``0x1C13`` (TxPDO). ESI parsing is out of scope.

   **Consequences.** ✅ ``EthercatRouting`` (:need:`REQ_0311`) becomes
   a compile-time-checked struct — bit offset, bit length, and PDO
   direction match the static map. ✅ No runtime XML parsing.
   ❌ Adding a new SubDevice model requires a code change, not a
   config-file swap. ❌ Out-of-tree SubDevices with unusual PDO
   assignments need manual mapping (acceptable — matches the rest of
   taktora's compile-time-config posture).

.. arch-decision:: Distributed Clocks bring-up is opt-in
   :id: ADR_0023
   :status: open
   :refines: REQ_0318

   **Context.** DC sub-microsecond synchronisation matters for motion
   control and time-stamped sampling; many EtherCAT deployments
   (digital I/O, ramped analog, slow process control) don't need it.
   DC bring-up adds a multi-pass register dance (BWR ``0x0900``,
   per-slave offset write to ``0x0920``, FRMW from ``0x0910``) and
   requires every SubDevice on the segment to declare 64-bit DC
   support.

   **Decision.** The gateway performs DC bring-up only when
   ``EthercatConnectorOptions::distributed_clocks`` is explicitly
   enabled by the application. Default is off.

   **Consequences.** ✅ Buses without DC-capable SubDevices work out
   of the box. ✅ Bring-up latency is lower when DC is unused.
   ❌ Motion-control applications must remember to enable DC.
   ❌ Two bring-up paths to test (with and without DC).

.. arch-decision:: Linux raw socket only in first cut
   :id: ADR_0024
   :status: open
   :refines: REQ_0325

   **Context.** ethercrab supports Linux raw sockets, NPCAP / WinPcap
   on Windows, and ``no_std`` direct-MAC drivers. Each adds porting
   work. EtherCAT in industrial deployments is overwhelmingly Linux;
   the production target is Linux.

   **Decision.** The first cut uses ethercrab's ``std::tx_rx_task``
   helper, which opens an ``AF_PACKET`` raw socket. Linux is the only
   supported host OS; the gateway process requires ``CAP_NET_RAW``.
   Windows and ``no_std`` MCU deployments are deferred.

   **Consequences.** ✅ One bring-up path to test in the first cut.
   ✅ Deployment recipe is "install the binary, grant CAP_NET_RAW".
   ❌ Windows-based engineering desks cannot run the gateway natively
   (they can run plugins; the gateway must live on Linux).
   ❌ Embedded MCU EtherCAT mainboards await a follow-on spec.

.. arch-decision:: ``taktora-connector-ethercat`` module decomposition
   :id: ADR_0025
   :status: open
   :refines: FEAT_0041

   **Context.** :need:`BB_0030` decomposes into plugin (:need:`BB_0031`),
   gateway (:need:`BB_0032`), PDO mapping (:need:`BB_0033`), and the
   tokio bridge (:need:`BB_0034`). An implementing crate can either
   place everything in one ``lib.rs`` (faster initial build, harder to
   navigate) or mirror the BB decomposition in module structure
   (one-to-one mapping to specs, slightly more setup).

   **Decision.** ``taktora-connector-ethercat`` mirrors the BB tree as
   sibling modules: ``plugin``, ``gateway``, ``pdo``, ``bridge``,
   ``options``, and ``health``. The public surface re-exports
   ``EthercatConnector`` from ``plugin``, ``EthercatGateway`` from
   ``gateway``, and ``EthercatConnectorOptions`` /
   ``EthercatRouting`` from ``options``. Internal modules are
   ``pub(crate)``.

   **Consequences.** ✅ Each module maps to one BB, so the
   ``IMPL_`` directive can refine its REQs at module granularity if
   future work needs finer-grained traceability. ✅ Test files
   under ``tests/`` align with module names. ❌ One more layer of
   directory nesting than the smaller framework crates currently
   adopt; acceptable because the connector crate is the largest.

.. arch-decision:: Tokio runtime owned by ``EthercatGateway``, joined on Drop
   :id: ADR_0026
   :status: open
   :refines: REQ_0321

   **Context.** :need:`REQ_0321` requires the ethercrab TX/RX task to
   run on a tokio runtime contained inside the connector crate, with
   no tokio leakage into taktora-executor's ``WaitSet`` thread. Three
   shapes are possible: (1) a global ``OnceCell<Runtime>`` shared
   across gateway instances, (2) a runtime owned per-``EthercatGateway``
   instance, joined on ``Drop``, (3) a runtime spawned externally and
   handed to the gateway via a builder.

   **Decision.** Each ``EthercatGateway`` instance owns its own
   ``tokio::runtime::Runtime`` (multi-threaded, defaulting to one
   worker thread, configurable via
   ``EthercatConnectorOptions::tokio_worker_threads``). The runtime
   is constructed in ``EthercatGateway::new`` and shut down via
   ``Runtime::shutdown_timeout`` in ``Drop`` with a 5-second budget
   (mirroring REQ_0244's SIGINT clean-exit budget).

   **Consequences.** ✅ Lifecycle is one-to-one with the gateway —
   no global state, multiple gateways on one host are independent.
   ✅ Mirrors :need:`ADR_0021` (one MainDevice per gateway).
   ❌ Spawning two gateways doubles the tokio worker-thread count;
   operators wanting a shared pool must consolidate gateways or wait
   for a follow-on spec.

.. arch-decision:: ``EthercatConnectorOptions`` is a typed builder; PDO map declared as ``&'static [SubDeviceMap]``
   :id: ADR_0027
   :status: open
   :refines: REQ_0314, REQ_0315

   **Context.** :need:`REQ_0314` requires the PDO mapping be declared
   by the application at build time via ``EthercatConnectorOptions``.
   Two builder shapes are common in Rust: (1) ``Default`` + public
   mutable fields, (2) a fluent typed builder with ``with_*``
   methods returning ``Self``. The PDO map itself can be a heap
   ``Vec<SubDeviceMap>`` or a ``&'static [SubDeviceMap]`` declared
   in application code.

   **Decision.** ``EthercatConnectorOptions`` is a typed builder
   (``EthercatConnectorOptions::builder()...with_subdevice(...).build()``)
   matching :need:`REQ_0270`'s ``ConnectorHost::builder()`` idiom.
   The PDO map is declared as ``&'static [SubDeviceMap]`` — held by
   reference so the application can place it in ``.rodata`` and the
   gateway needs no per-instance heap allocation for it. Individual
   ``SubDeviceMap`` entries reference ``&'static [PdoEntry]`` for the
   same reason.

   **Consequences.** ✅ No heap allocation for the PDO map after
   gateway construction (consistent with taktora-executor's REQ_0060
   posture for the steady-state hot path). ✅ Builder API parallel to
   the framework's other connector options. ❌ Applications that need
   runtime-discovered PDO maps (e.g. EEPROM-parsed) must roll their
   own ``&'static`` storage or wait for a runtime-PDO follow-on spec.

.. arch-decision:: Verification harness — pure-logic unit tests + env-gated bus tests
   :id: ADR_0028
   :status: open
   :refines: FEAT_0041

   **Context.** :need:`FEAT_0041` ships 16 TEST artefacts
   (TEST_0200..TEST_0215) verifying REQ_0310..REQ_0325. Six of those
   tests (TEST_0203, TEST_0205, TEST_0208, TEST_0209, TEST_0210,
   TEST_0215) exercise real bus state transitions, PDO mapping
   application, working-counter accounting, DC bring-up, or raw
   socket access — operations that need either an ``ethercrab``
   ``MainDevice`` driving a real NIC or a mock that simulates
   SubDevice responses. An earlier draft of this ADR assumed
   ``ethercrab`` shipped a ``MockMainDevice``; it does not (as of
   ``ethercrab`` 0.7), so the verification strategy below is the
   actual approach taken.

   **Decision.** The connector's testable logic is factored into
   pure-Rust modules — :need:`IMPL_0050`'s ``sdo`` (SDO write
   sequence generation), ``scheduler`` (cycle-time pacing with
   skip-not-catch-up semantics), ``wkc`` (working-counter health
   policy), ``bridge`` (bounded outbound / inbound bridges),
   ``health`` (health monitor + broadcast), ``options`` (typed
   builder with default-clamp), and ``routing`` — and unit-tested
   deterministically without ``ethercrab`` on the wire (TEST_0201,
   TEST_0204, TEST_0205-partial, TEST_0206, TEST_0207, TEST_0209,
   TEST_0210, TEST_0211-partial, TEST_0212, TEST_0213, TEST_0214 all
   land via this path). The remaining bus-driven tests
   (TEST_0202, TEST_0203, TEST_0205-full, TEST_0208 wire-side,
   TEST_0211-full, TEST_0215) live in
   ``crates/taktora-connector-ethercat/tests`` and are gated on the
   ``ETHERCAT_TEST_NIC`` environment variable; absent the variable
   they ``skip!`` rather than failing. CI runs the pure-logic tests
   on every push; the bus suite runs only on the gateway host
   (Linux + CAP_NET_RAW) as a manual workflow.

   **Consequences.** ✅ Every PR build is green on every developer
   machine and CI runner — no flaky "missing NIC" failures.
   ✅ The factored pure-logic modules (``sdo`` / ``scheduler`` /
   ``wkc``) carry the gateway's load-bearing decision logic and are
   exhaustively tested. ✅ The bus suite still exists in-tree and is
   one ``ETHERCAT_TEST_NIC=eth0`` away from running. ❌ The bus
   tests are not on the CI gate; a regression that only surfaces on
   real hardware will only be caught when the gateway host runs the
   suite — documented as an accepted risk. ❌ Without a mock, the
   bridge between ``ethercrab``'s ``MainDevice`` API and the
   pure-logic helpers is itself untested at unit level; a follow-on
   may introduce a trait abstraction (``BusDriver`` with a
   ``MockBusDriver`` impl in ``dev-dependencies``) once the
   integration surface is stable enough for the abstraction not to
   churn.

.. arch-decision:: Zenoh queries live on a concrete handle type, not the Connector trait
   :id: ADR_0040
   :status: open
   :refines: FEAT_0044

   **Context.** The framework explicitly rejected protocol-portable
   channels (:need:`REQ_0294`) and framework-level request/response
   matching (:need:`REQ_0290`). Three options for surfacing Zenoh
   queries existed: (a) concrete methods on ``ZenohConnector`` only;
   (b) extend the ``Connector`` trait with default-noop query methods;
   (c) re-use pub/sub plus app-level correlation.

   **Decision.** Option (a). ``ZenohConnector::create_querier`` and
   ``ZenohConnector::create_queryable`` are concrete methods that
   return Zenoh-specific handle types (``ZenohQuerier``,
   ``ZenohQueryable``). The shared ``Connector`` trait remains
   unchanged.

   **Consequences.** ✅ Honors :need:`REQ_0290` / :need:`REQ_0294`. ✅
   MQTT and EtherCAT connectors are not forced to invent
   no-op query plumbing. ❌ Plugin code wanting queries depends on
   the concrete ``ZenohConnector`` type, not the abstract trait —
   but that is exactly the framework's existing posture for
   protocol-specific affordances (:need:`REQ_0224`).

.. arch-decision:: Stack-internal reconnect for Zenoh — no ReconnectPolicy
   :id: ADR_0041
   :status: open
   :refines: FEAT_0045

   **Context.** Zenoh's own session machinery handles scout and
   reconnect (peer mode) and reconnect-to-router (client mode). The
   framework provides :need:`REQ_0232` ``ReconnectPolicy`` and a
   default ``ExponentialBackoff``, but also explicitly allows
   stack-internal-reconnect connectors to skip it
   (:need:`REQ_0235`).

   **Decision.** The Zenoh connector follows the
   stack-internal-reconnect path. ``ReconnectPolicy`` is not used;
   the gateway observes the Zenoh session's alive/closed state and
   emits ``HealthEvent`` on every transition. An anti-req
   :need:`REQ_0441` records the decision in the requirements page.

   **Consequences.** ✅ No duplicate retry policy contending with
   Zenoh's own. ✅ Health emission stays uniform across all
   connectors (:need:`REQ_0234`). ❌ If a future user wants
   ``zenoh::open`` itself retried with backoff on initial config
   failure, that becomes a follow-on req — current behavior is to
   return ``Down`` and rely on application-level restart.

.. arch-decision:: One ZenohRouting struct carries pub/sub QoS; query knobs on options
   :id: ADR_0042
   :status: open
   :refines: FEAT_0043

   **Context.** :need:`REQ_0224` already declares that each
   connector ships a single routing struct (``MqttRouting``,
   ``EthercatRouting``, ``ZenohRouting``) implementing the
   ``Routing`` marker. Zenoh has both pub/sub QoS knobs
   (congestion control, priority, reliability, express) and
   query-specific knobs (target, consolidation, timeout). Two
   options: (a) one routing struct carrying pub/sub QoS, with
   query knobs on ``ZenohConnectorOptions``; (b) two distinct
   routing structs.

   **Decision.** Option (a). ``ZenohRouting`` carries
   ``{ key_expr, congestion_control, priority, reliability,
   express }``. Query-specific knobs (target, consolidation,
   timeout) live on ``ZenohConnectorOptions`` as session-wide
   defaults; ``ZenohQuerier`` exposes a builder to override the
   timeout per-call.

   **Consequences.** ✅ Preserves :need:`REQ_0224`'s single-routing-
   struct rule. ✅ Mirrors :need:`REQ_0251` (MQTT carries QoS in
   routing). ❌ Per-channel query target / consolidation overrides
   require a builder method instead of a routing field — accepted
   tradeoff for type-system simplicity.

.. arch-decision:: Startup SDOs as a typed SubDeviceMap field
   :id: ADR_0103
   :status: accepted
   :refines: REQ_0853

   **Context.** Device configuration that must precede PDO assignment
   (motor current, operation-mode selection) has to be written before the
   ``0x1C12``/``0x1C13`` PDO assignment, while the SubDevice is still in
   PRE-OP.

   **Decision.** Such configuration is declared as a static
   ``startup_sdos`` slice on ``SubDeviceMap``, applied in PRE-OP before the
   ``0x1C12``/``0x1C13`` assignment writes, rather than exposed via a
   runtime SDO escape hatch.

   **Consequences.** ✅ Bring-up stays fully declarative and reproducible
   from the checkout (consistent with ``with_sm_watchdog`` and explicit
   WKC). ✅ The same data drives the planned master-side motion runtime.
   ❌ Only ``SdoValue`` types (U8/U16) are expressible today; wider types
   land when a device needs them.

.. arch-decision:: Reply framing uses a Zenoh-private 1-byte payload prefix
   :id: ADR_0043
   :status: open
   :refines: FEAT_0044

   **Context.** Multi-reply Zenoh queries need an end-of-stream
   signal in addition to data chunks. Two options: (a) allocate
   one bit of ``ConnectorEnvelope``'s reserved word
   (:need:`REQ_0200`) — but that turns the reserved word into
   Zenoh-specific framework metadata; (b) carry a one-byte frame
   discriminator inside ``envelope.payload[0]`` — Zenoh-private,
   the framework remains agnostic.

   **Decision.** Option (b). Every envelope on the two reply-side
   iceoryx2 services (``{name}.reply.in`` / ``{name}.reply.out``)
   begins ``payload`` with a 1-byte discriminator: ``0x01`` = data
   chunk (followed by codec-encoded ``R``), ``0x02`` = end of
   stream (empty body), ``0x03`` = gateway-synthetic timeout
   (empty body). The framework's reserved word stays untouched.

   **Consequences.** ✅ Framework anti-goal (no inspection of
   envelope payload, no protocol-portable semantics in the
   reserved word) preserved. ✅ Future connectors can re-use the
   pattern without coordinating with the framework. ❌ Plugin-side
   ``ZenohQuerier::try_recv`` and ``ZenohQueryable::reply`` add a
   single-byte skip / write step relative to pub/sub channels.

.. arch-decision:: UI connector is a passive, MVVM-shaped, language-neutral transport
   :id: ADR_0107
   :status: accepted
   :refines: FEAT_0092

   **Context.** A local human-machine interface needs to observe and
   command a taktora-executor application without binding taktora to
   any one UI toolkit or runtime. The grill that produced
   :need:`FEAT_0092` resolved five tightly-coupled forks, each with a
   rejected alternative: (1) the boundary's reach — Rust-GUI-only vs
   language/runtime-agnostic; (2) who builds the ViewModel — a passive
   transport the app pushes into vs an active aggregator that reaches
   into executor/stats/other connectors; (3) the substrate — desugar
   onto the existing ``Connector`` trait vs a parallel
   ``ViewModelHost`` abstraction; (4) the publish granularity and RT
   posture — struct-per-ViewModel over a telemetry-export-style
   seqlock+pump split vs service-per-property or publishing from the
   RT task; (5) command semantics — acceptance-ack request/response vs
   fire-and-forget or a held-open completion future. These are not
   independent: the language-neutral choice forces a self-describing
   wire contract, which (with RT-safety) forces fixed-layout POD
   fields, which makes struct-per-ViewModel and a seqlock cell the
   natural fit.

   **Decision.** Adopt the following bundled posture for the UI
   connector, captured as the requirements cited:

   * **Language/runtime-agnostic, manifest-normative
     (:need:`REQ_0872`–:need:`REQ_0878`).** The normative boundary is
     a self-describing manifest plus a closed, fixed-layout POD field
     type system (:need:`REQ_0858` / :need:`REQ_0875`) carried as JSON
     over iceoryx2, with a contract hash (:need:`REQ_0874`). Any
     language with an iceoryx2 binding binds dynamically; a Rust
     reference client is the v1 ergonomic consumer; per-language typed
     codegen is deferred.
   * **Passive transport, not an aggregator (:need:`FEAT_0093`).** The
     connector knows nothing of taktora's internal types; the
     application owns the ViewModel binding glue. Standard-source
     bridges (executor ``Observer`` telemetry, connector health) are
     reserved for optional ``-adapters`` crates, keeping the core
     decoupled — consistent with telemetry-export reaching the
     executor through a separate ``Observer`` seam rather than a
     connector.
   * **Desugared onto the ``Connector`` trait (:need:`REQ_0855`).**
     ``UiConnector`` implements the shared trait
     (``Routing = UiRouting``, ``Codec = C``, ``JsonCodec`` default);
     ``Property`` / ``Command`` / ``CanExecute`` are additive
     ergonomics over ``create_writer`` / ``create_reader``. The trait
     is not modified — mirroring :need:`ADR_0040`'s posture for Zenoh
     queries.
   * **Struct-per-ViewModel over a seqlock + non-RT pump
     (:need:`REQ_0856`, :need:`REQ_0860`, :need:`REQ_0861`).** A
     ViewModel is one iceoryx2 service publishing one POD struct with
     latest-value (history-depth-1) semantics. The producer writes a
     seqlock cell on the (possibly RT) hot path with no allocation; a
     non-RT pump snapshots, JSON-encodes off-RT, and publishes at a
     configurable UI cadence with coalescing — the
     ``taktora-telemetry-export`` discipline (:need:`FEAT_0038`)
     applied to bidirectional UI state.
   * **Acceptance-ack commands, at-most-once (:need:`REQ_0865`,
     :need:`REQ_0867`, :need:`REQ_0868`).** Commands are
     request/response keyed by ``correlation_id`` returning
     ``Accepted`` / ``Rejected``; progress and outcome are observed
     through properties, not a held-open call. Delivery is
     at-most-once via dedupe, with opt-in auto-retry only for commands
     flagged idempotent.

   **Consequences.** ✅ A WPF/.NET, web, C++, or Python UI can bind off
   the manifest on day one — the stated goal — without taktora shipping
   N language generators. ✅ The deterministic executor is never
   encoded on, blocked, or back-pressured by a UI consumer
   (:need:`REQ_0860` / :need:`REQ_0870`). ✅ Health, lifecycle,
   ``ConnectorHost`` registration, envelope, and codec are inherited,
   not re-solved, by being a first-class ``Connector``. ✅ The POD-only
   constraint makes each ViewModel's envelope ``N`` derivable by the
   derive macro (:need:`REQ_0859`) and the wire layout trivial for
   every language. ❌ Property loses per-field independence — a
   "property" is a field of a published struct, so per-field change is
   reconstructed by client-side diff (:need:`REQ_0864`) and per-field
   metadata (timestamps, staleness) must be modeled inside the struct.
   ❌ POD-only fields exclude heap/variable-length state and
   data-carrying tagged unions in v1 (:need:`REQ_0858`); enums that
   carry data become an authoring pattern, not a language feature. ❌
   Lossy coalescing suits state but not alarms/audit, so a lossless
   ``Event`` primitive is deferred to v2 with a reserved ``kind`` slot
   (:need:`REQ_0857`). ❌ Trust is OS/iceoryx2-mediated only in v1
   (:need:`REQ_0884`).
