EtherCAT integration tests
==========================

EtherCAT tests come in two flavours: software tests (unit + raw-frame
mock) parallel-safe on any host, and hardware-in-the-loop tests marked
``[bench]`` that require an EK1100 + EL-series fixture on the CI test
bench. The mock implementation lives in
``taktora-connector-ethercat/tests/mock/`` and replays canned EtherCAT
frame responses so bring-up, PDO mapping, and WKC scenarios can be
exercised without hardware. Bench tests run only when invoked as
``cargo test --features ethercat-bench``.

.. test:: EthercatConnector trait surface
   :id: TEST_0200
   :status: open
   :verifies: REQ_0310

   Compile-time test confirming that
   ``EthercatConnector<JsonCodec>`` implements ``Connector`` with
   ``type Routing = EthercatRouting``. A ``trybuild`` compile-fail
   companion test asserts that swapping ``Routing`` to a foreign
   marker fails to compile.

.. test:: EthercatRouting field round-trip
   :id: TEST_0201
   :status: open
   :verifies: REQ_0311

   Unit test constructing ``EthercatRouting`` values with the four
   fields (SubDevice configured address, PDO direction, bit offset,
   bit length); asserts the values round-trip through serialization
   and that ``EthercatRouting: Routing + Clone + Send + Sync + Debug
   + 'static`` holds at compile time.

.. test:: Single MainDevice per gateway instance
   :id: TEST_0202
   :status: open
   :verifies: REQ_0312

   Construct an ``EthercatGateway`` builder and confirm that the
   builder accepts exactly one network interface name and produces
   exactly one ``MainDevice``. A second call to ``.with_interface``
   replaces the previous value rather than producing a second device.

.. test:: Bus reaches OP before traffic accepted
   :id: TEST_0203
   :status: open
   :verifies: REQ_0313

   Mock-frame test: start an ``EthercatGateway`` against the
   raw-frame mock; before the mock acknowledges the SAFE-OP → OP
   transition, ``ChannelWriter::send`` from the plugin side returns
   ``ConnectorError::NotReady``. After the mock signals OP, the same
   send completes successfully.

.. test:: Static PDO map accepted from options
   :id: TEST_0204
   :status: open
   :verifies: REQ_0314

   Unit test that ``EthercatConnectorOptions::with_pdo_mapping``
   accepts a static description per SubDevice (RxPDO / TxPDO
   entries) and that the in-memory representation preserves entry
   order and bit-offset values. Mismatched declarations (bit length
   exceeds SubDevice capacity) are rejected at builder time.

.. test:: PDO mapping applied during PRE-OP to SAFE-OP
   :id: TEST_0205
   :status: open
   :verifies: REQ_0315

   Mock-frame test observing the SDO write sequence during bring-up:
   the gateway emits writes to ``0x1C12`` (RxPDO) and ``0x1C13``
   (TxPDO) before the SAFE-OP transition is requested. The exact
   sub-index sequence matches the configured PDO mapping.

.. test:: Cycle time configurable
   :id: TEST_0206
   :status: open
   :verifies: REQ_0316

   Unit test that ``EthercatConnectorOptions::cycle_time`` accepts
   ``Duration`` values from 1 ms upward; the default is 2 ms; values
   below 1 ms are rejected at builder time. The configured value is
   observable on the gateway's metadata accessor.

.. test:: Missed ticks are skipped not queued
   :id: TEST_0207
   :status: open
   :verifies: REQ_0317

   Mock-frame test: stall the gateway's tokio sidecar for 5 cycles
   by holding a mutex on the bridge. After release, exactly one
   tx_rx cycle runs (not five) — the skipped ticks are dropped, not
   queued for catch-up. The asserted behaviour matches
   ``tokio::time::MissedTickBehavior::Skip``.

.. test:: Distributed Clocks bring-up is opt-in
   :id: TEST_0208
   :status: open
   :verifies: REQ_0318

   Two mock-frame scenarios. (1) Default options: the gateway
   completes bring-up without emitting any BWR to ``0x0900``, FRMW
   to ``0x0910``, or write to ``0x0920``. (2)
   ``distributed_clocks: true``: the DC register sequence appears
   between PRE-OP and SAFE-OP exactly once per bring-up.

.. test:: Up requires OP and matching working counter
   :id: TEST_0209
   :status: open
   :verifies: REQ_0319

   Mock-frame test: drive the gateway to OP with the mock reporting
   the expected WKC on every cycle for 10 consecutive cycles;
   ``Connector::health()`` returns ``ConnectorHealth::Up``. Inject a
   single low WKC cycle and immediately query health; ``Up`` is no
   longer reported.

.. test:: Working-counter mismatch transitions to Degraded
   :id: TEST_0210
   :status: open
   :verifies: REQ_0320

   Mock-frame test: configure a degradation threshold of N=3 cycles;
   inject N consecutive cycles with WKC below the expected value;
   the gateway transitions to ``ConnectorHealth::Degraded`` and
   emits exactly one ``HealthEvent::Degraded`` with a reason naming
   the offending cycle count.

.. test:: Tokio sidecar contained inside connector crate
   :id: TEST_0211
   :status: open
   :verifies: REQ_0321

   Structural test using ``cargo tree``: assert that
   ``taktora-executor`` does not appear with ``tokio`` in its
   transitive dependency closure, and that ``tokio`` appears only
   under ``taktora-connector-ethercat`` (and other connector crates).
   A second assertion: the published ``EthercatConnector`` plugin
   surface contains no ``tokio::`` types.

.. test:: Bridge channels are bounded with configurable capacity
   :id: TEST_0212
   :status: open
   :verifies: REQ_0322

   Unit test that ``EthercatConnectorOptions::outbound_capacity``
   and ``inbound_capacity`` produce bridges with exactly the
   configured number of slots. After filling the channel, further
   non-blocking sends return ``Full`` rather than allocating
   additional capacity.

.. test:: Outbound bridge saturation surfaces as BackPressure
   :id: TEST_0213
   :status: open
   :verifies: REQ_0323

   Mock-frame test: configure a tiny outbound-bridge capacity. Stall
   the tokio sidecar from draining. The plugin's ``ChannelWriter::send``
   returns ``ConnectorError::BackPressure`` and the gateway reports
   ``ConnectorHealth::Degraded`` until the bridge drains.

.. test:: Inbound bridge saturation surfaces as DroppedInbound
   :id: TEST_0214
   :status: open
   :verifies: REQ_0324

   Mock-frame test: configure a tiny inbound-bridge capacity. Block
   the inbound gateway item from draining. Drive a flood of inbound
   process-image updates through the mock; the gateway emits one or
   more ``HealthEvent::DroppedInbound { count }`` and the inbound
   image for affected cycles is dropped (not buffered).

.. test:: Gateway opens raw socket on Linux with CAP_NET_RAW
   :id: TEST_0215
   :status: open
   :verifies: REQ_0325

   Hardware-bench test ``[bench]``: on a Linux CI host with
   ``CAP_NET_RAW`` granted to the test binary, the gateway opens the
   configured NIC via ``ethercrab::std::tx_rx_task`` and reports
   ``Up``. Companion negative test (also Linux): when
   ``CAP_NET_RAW`` is absent, gateway startup fails with a
   permission error before any EtherCAT frame is sent.

.. test:: PDI bit-slice byte-aligned round-trip
   :id: TEST_0216
   :status: open
   :verifies: REQ_0326, REQ_0327

   Pure-logic test of the ``pdi`` module. For a representative
   set of ``(bit_offset, bit_length)`` pairs where both endpoints
   are byte-aligned (``bit_offset % 8 == 0``,
   ``bit_length % 8 == 0``), the round-trip
   ``write_routing(buf, routing, value); read_routing(buf, routing)``
   shall yield the original ``value`` byte-for-byte, with no
   modification to PDI bytes outside the slice. Property test via
   ``proptest`` over slice positions, lengths, and pre-existing
   buffer contents.

.. test:: PDI bit-slice unaligned round-trip
   :id: TEST_0217
   :status: open
   :verifies: REQ_0326, REQ_0327

   Property test for the same round-trip as :need:`TEST_0216` but
   covering ``bit_offset`` and ``bit_length`` values that are not
   multiples of 8. Verifies that read-modify-write on partial
   leading / trailing bytes preserves the unaffected bits exactly
   (no spillover into adjacent slices).

.. test:: Adjacent PDI bit slices do not interfere
   :id: TEST_0218
   :status: open
   :verifies: REQ_0326

   Construct two ``EthercatRouting`` declarations whose bit slices
   are adjacent (e.g. slice A = bits 0..12, slice B = bits 12..24
   on the same SubDevice / direction). Write distinct values to A
   and B in arbitrary order; read both back. Both reads shall
   return the original written values; neither write shall corrupt
   the other slice.

.. test:: Per-channel routing registry has stable iteration order
   :id: TEST_0219
   :status: open
   :verifies: REQ_0328

   When the application registers N channel descriptors in order
   ``D_1 … D_N`` via ``create_writer`` / ``create_reader``, the
   gateway's cycle-loop iteration over the registry shall visit
   them in the same order on every cycle. Property test confirms
   the order is stable across 1 000 cycles and zero per-cycle
   allocations are observed via ``CountingAllocator``.

.. test:: Outbound end-to-end (plugin send → PDI slice via mock)
   :id: TEST_0220
   :status: open
   :verifies: REQ_0326, REQ_0328

   With a ``MockBusDriver`` configured for a single SubDevice at
   address ``0x0001`` with an outputs buffer of N bytes, a plugin
   that constructs an ``EthercatConnector`` and calls
   ``create_writer`` for a ``ChannelDescriptor`` whose
   ``EthercatRouting`` selects bits ``[bit_offset, bit_offset +
   bit_length)`` of that SubDevice's outputs, then invokes
   ``ChannelWriter::send(value)``: after the next cycle, the
   mock's outputs buffer at the routing's bit slice shall equal
   the codec-encoded representation of ``value``. PDI bytes
   outside the routing's slice shall remain unchanged.

.. test:: Inbound end-to-end (PDI slice via mock → plugin recv)
   :id: TEST_0221
   :status: open
   :verifies: REQ_0327, REQ_0328

   With a ``MockBusDriver`` preloaded with inputs bytes at a known
   bit slice via ``with_subdevice_inputs``, a plugin that
   constructs an ``EthercatConnector`` and calls ``create_reader``
   for a routing pointing at that slice: after one cycle,
   ``ChannelReader::try_recv()`` shall return an envelope whose
   decoded payload equals the value the mock's preloaded bytes
   represent under the channel's codec.

.. test:: Loopback round-trip (plugin → mock → plugin)
   :id: TEST_0222
   :status: open
   :verifies: REQ_0326, REQ_0327

   Compose :need:`TEST_0220` and :need:`TEST_0221` via a ``MockBusDriver``
   variant that, on every cycle, copies the SubDevice's outputs
   buffer over to its inputs buffer (synthetic loopback). The
   plugin registers paired Rx and Tx channels pointing at the
   same bit slice and a fresh routing pair; after one cycle of
   ``ChannelWriter::send(v)`` + ``ChannelReader::try_recv()``,
   the received value equals ``v`` byte-for-byte. Verifies
   end-to-end iceoryx2 ↔ PDI ↔ iceoryx2 plumbing without
   hardware.

.. test:: Asymmetric expected_wkc summing
   :id: TEST_0223
   :status: open
   :verifies: REQ_0329

   Table-driven unit test in
   ``crates/taktora-connector-ethercat-tests/tests/recovery.rs``
   asserting ``BringUp.expected_wkc`` equals the per-SubDevice
   ``expected_wkc`` sum over SubDevices present in ``pdo_map``,
   with bus-only SubDevices contributing 0.

.. test:: DC cycle path branches on options.distributed_clocks
   :id: TEST_0224
   :status: open
   :verifies: REQ_0330

   Mock-driven unit test asserting ``MockBusDriver`` records
   ``CycleKind::Dc`` exactly when ``options.distributed_clocks()``
   is ``true``, and ``CycleKind::Plain`` otherwise. Lives in
   ``crates/taktora-connector-ethercat-tests/tests/recovery.rs``.

.. test:: Recovery state machine drives BusDriver::recover per policy
   :id: TEST_0225
   :status: open
   :verifies: REQ_0331, REQ_0332

   Integration test in
   ``crates/taktora-connector-ethercat-tests/tests/recovery.rs``
   composing ``MockBusDriver`` (programmed recovery sequence) +
   ``CycleRunner`` + ``ExponentialBackoff::with_max_attempts(N)``.
   Cases: one recoverable fault, multiple in sequence (fresh policy
   per episode), policy exhausted ⇒ terminal Down + runner exits,
   recovery yields a new expected_wkc adopted on the next cycle.

.. test:: Health transitions during recovery
   :id: TEST_0226
   :status: open
   :verifies: REQ_0333

   Integration test asserting the exact emitted ``HealthEvent``
   sequence across cycle-error → backoff → recover → up. Uses the
   broadcast channel exposed by ``EthercatHealthMonitor``. Lives in
   ``crates/taktora-connector-ethercat-tests/tests/recovery.rs``.

.. test:: Hardware drill — endurance + unplug/replug
   :id: TEST_0227
   :status: open
   :verifies: REQ_0331

   Manual hardware test against EK1100 + EL1008 + EL2004. Operator
   runs ``examples/ethercat-real-bus --mode drill --window 60`` and
   ``--mode endurance --duration 3600``, performs the unplug/replug
   drill, and archives the full stderr capture as
   ``docs/superpowers/specs/2026-05-28-ethercrab-bus-driver-drill.log``.
   Pass criterion: zero terminal ``Down`` transitions in endurance,
   ``Up → Degraded → Connecting → Up`` sequence within the policy's
   backoff envelope during the drill.

.. test:: OP wait-loop pacing decisions
   :id: TEST_0857
   :status: open
   :verifies: REQ_0841

   Unit tests in
   ``crates/taktora-connector-ethercat/tests/op_transition.rs`` over
   the pure ``op_wait_action`` decision: regular spins continue,
   every ``OP_WAIT_ACK_INTERVAL``-th spin acknowledges latched AL
   errors, spins beyond ``OP_WAIT_MAX_SPINS`` give up. The
   cyclic-exchange walk itself is hardware-verified via the
   ``#[ignore]``-gated ``tests/ethercrab_driver.rs`` bring-up test
   run against a WAGO 750-354 — the SM-watchdog coupler that
   motivated the requirement.

.. test:: Bring-up failure surfaces as terminal Down
   :id: TEST_0858
   :status: open
   :verifies: REQ_0842

   Integration test in
   ``crates/taktora-connector-ethercat-tests/tests/bring_up_failure.rs``:
   an ``EthercatConnector`` over ``MockBusDriver::failing_bring_up``
   is registered with an executor; the health subscription must
   observe exactly ``Connecting → Down`` with a reason carrying both
   the ``"bring-up failed"`` prefix and the driver's error text —
   instead of the connector idling in ``Connecting`` forever.

.. test:: SM-watchdog tick maths against the AOU_0016 bound
   :id: TEST_0862
   :status: open
   :verifies: REQ_0846

   Unit tests in
   ``crates/taktora-connector-ethercat/tests/watchdog.rs`` over the
   pure ``SmWatchdog`` register model: the default divider yields a
   100 µs tick (``2498 → 40 ns × 2500``); ``from_timeout_us(50_000)``
   gives 500 ticks, an effective window of exactly 50 ms; the ceil
   quantization rounds a 50_001 µs request up to 501 ticks (50.1 ms,
   above the FTTI/2 bound — so callers must check the *effective*
   value); a 0 µs request clamps up to one tick (never a disabling
   0-interval); and an enormous request saturates at ``u16::MAX``
   ticks without overflow. The register write plus read-back-verify
   path is hardware-gated: the ``#[ignore]``-gated
   ``tests/ethercrab_driver.rs`` bring-up test programs a 50 ms window
   on an output SubDevice (configured address from
   ``ETHERCAT_TEST_WD_ADDRESS``) and treats reaching OP as evidence
   that the registers stuck, since a read-back mismatch fails
   bring-up.

.. test:: SM-watchdog safe-state drill on real WAGO hardware
   :id: TEST_0863
   :status: open
   :verifies: REQ_0846, REQ_0331

   Manual hardware drill against the WAGO 750-354 + 750-602 + 750-430
   + 750-530 rig, executed 2026-06-07 via
   ``examples/ethercat-wago-coupler --mode drill``; full stderr/stdout
   capture archived as
   ``docs/superpowers/specs/2026-06-07-wago-sm-watchdog-drill.log``.
   Observed end to end: bring-up reaches ``Up`` with the 50 ms window
   programmed and read-back-verified (a mismatch hard-fails, so
   ``Up`` is the register evidence); on cable unplug with a 24 V
   input held, the mirrored 750-530 output drops to safe state (SM
   watchdog fires) while the input stays high; the recovery loop
   retries with honest per-attempt reasons across seven failed
   attempts — the ``MainDevice`` surviving every failure per
   :need:`REQ_0331`'s retryability clause; on replug, recovery
   re-programs and re-verifies the watchdog and returns to ``Up``;
   the application re-commands its outputs on observing the ``Up``
   transition (one scan later). Drill verdict:
   ``saw_degraded=true saw_recover_up=true``.

.. test:: Health subscriptions broadcast to every subscriber
   :id: TEST_0864
   :status: open
   :verifies: REQ_0847

   Per-connector integration tests in
   ``crates/taktora-connector-{ethercat,can,zenoh}/tests/health_broadcast.rs``:
   two subscriptions opened before a transition must BOTH observe it
   (the regression from issue #60 — under the old shared-receiver
   implementation exactly one of them stole the event); a
   subscription opened after a transition must NOT replay
   pre-subscription history; and a transition with zero subscribers
   must succeed and remain observable via ``current()``.

.. test:: Startup SDOs written in order before PDO assignment
   :id: TEST_0869
   :status: open
   :verifies: REQ_0853

   Unit tests in ``taktora-connector-ethercat`` over the startup-SDO
   sequencer:
   ``startup_writes_carry_map_address_and_order`` asserts that, for a
   ``SubDeviceMap`` carrying a non-empty ``startup_sdos`` list, the driver
   emits one SDO write per ``StartupSdo`` to the matching SubDevice address
   in declaration order, and that every startup write precedes the
   ``0x1C12``/``0x1C13`` PDO-assignment writes of :need:`REQ_0315`;
   ``empty_startup_sdos_produce_no_writes`` asserts that a ``SubDeviceMap``
   with an empty ``startup_sdos`` list produces zero startup SDO writes.
