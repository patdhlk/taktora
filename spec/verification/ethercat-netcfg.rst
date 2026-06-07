EtherCAT network-config codegen — verification
===============================================

Test cases verifying the EtherCAT network-config codegen toolchain.
Each ``test`` directive ``:verifies:`` one or more requirements from
:doc:`../requirements/ethercat-netcfg`.

The toolchain is build-time only. As with :doc:`device-codegen`, the
verification surface is heavier on snapshot / golden-file / property
tests and build-script behaviour than on multi-process integration; the
generated tables are exercised end-to-end against real hardware by the
``examples/ethercat-*`` integration examples, which this page does not
duplicate.

----

Parser and IR tests
-------------------

Per-crate, no I/O beyond test fixtures, parallel-safe. Live under
``crates/ethercat-netcfg/tests/``.

.. test:: parse() accepts a representative WAGO network.yaml
   :id: TEST_0830
   :status: open
   :verifies: REQ_0820, REQ_0821

   Loads a canonical ``network.yaml`` fixture describing a WAGO 750-354
   coupler with two channels, calls the parse entry point, and asserts
   the resulting ``NetworkConfig`` carries the expected ``BusConfig``
   (cycle time, ``max_subdevices`` / ``max_pdi_bytes``), one
   ``DeviceInstance`` with its ``label`` and ``DeviceSource``, and two
   ``ChannelBinding`` entries with the expected direction, bit offset,
   and bit length.

.. test:: Multi-bus document is rejected
   :id: TEST_0831
   :status: open
   :verifies: REQ_0822

   A fixture declaring two top-level buses in one document parses to an
   error naming the one-file-one-bus rule. A single-bus fixture parses
   without error. Guards the scope boundary of :need:`ADR_0096`.

.. test:: Channels resolve to devices by label, stable under reorder
   :id: TEST_0832
   :status: open
   :verifies: REQ_0823

   A fixture whose channels reference devices by ``label`` parses
   correctly; reordering the device list in the fixture (without editing
   any channel) leaves every channel bound to the same device. Confirms
   bindings key on ``label``, not list index or address.

.. test:: Parser is independent of the connector runtime
   :id: TEST_0833
   :status: open
   :verifies: REQ_0824

   ``cargo tree -p ethercat-netcfg`` shall not list
   ``taktora-connector-ethercat`` anywhere in the resolved graph.
   Implemented as a CI shell check that greps the output and fails on
   match. ``ethercat-esi`` and ``fieldbus-od-core`` are expected
   present.

----

Codegen tests
-------------

Per-crate, snapshot-based. Live under
``crates/ethercat-netcfg-codegen/tests/``.

.. test:: Generated module emits the expected static PDO_MAP
   :id: TEST_0834
   :status: open
   :verifies: REQ_0825

   Golden-file test: codegen over the WAGO fixture produces a
   ``pub static PDO_MAP: &[SubDeviceMap]`` whose single entry carries the
   computed address, the mapped ``PdoEntry`` slices, and the derived
   ``expected_wkc``. Snapshot compared against a checked-in golden module.

.. test:: Generated routing and channel-name constants match the bindings
   :id: TEST_0835
   :status: open
   :verifies: REQ_0826

   For each channel binding in the fixture, the generated module exposes
   a named ``EthercatRouting`` constant carrying the resolved subdevice
   address, direction, bit offset, and bit length, plus the channel-name
   string constant. Asserted against the golden snapshot.

.. test:: Configured addresses follow bus position, override honoured
   :id: TEST_0836
   :status: open
   :verifies: REQ_0827

   A three-device fixture generates addresses ``0x1000``, ``0x1001``,
   ``0x1002`` in list order. A second fixture with an explicit
   ``address: 0x1005`` override on the middle device generates
   ``0x1000``, ``0x1005``, ``0x1002``. Confirms positional assignment and
   the escape hatch of :need:`ADR_0093`.

.. test:: expected_wkc is derived from PDO directions
   :id: TEST_0837
   :status: open
   :verifies: REQ_0828

   Property test over fixtures: a TxPDO-only device generates
   ``expected_wkc = 2``, an RxPDO-only device ``1``, a both-directions
   device ``3``, and a PDO-less coupler ``0`` — the canonical 0/1/2/3
   rule. No fixture is able to express an override value; the schema
   carries no such field (guards :need:`ADR_0095`).

.. test:: Generated output is byte-deterministic
   :id: TEST_0838
   :status: open
   :verifies: REQ_0829

   Codegen runs twice over the same fixture and pinned ESI inputs; the
   two emitted modules are byte-identical. A variant reorders unrelated
   YAML keys and confirms the output is unchanged. No timestamp or
   hash-map iteration order leaks into the file.

----

Build-script tests
------------------

Integration tests of the ``build.rs`` glue. Live under
``crates/ethercat-netcfg-build/tests/``.

.. test:: Build helper generates into OUT_DIR and the module compiles
   :id: TEST_0839
   :status: open
   :verifies: REQ_0830

   A throwaway consumer crate whose ``build.rs`` invokes the helper over
   a fixture compiles successfully while pulling the module in via
   ``include!(concat!(env!("OUT_DIR"), "/network.rs"))``. The generated
   file is confirmed absent from the source tree.

.. test:: Build helper emits rerun-if-changed for config and ESI
   :id: TEST_0840
   :status: open
   :verifies: REQ_0831

   Capturing the build-script stdout, the test asserts a
   ``cargo:rerun-if-changed`` line for the ``network.yaml`` and for each
   vendored ESI file the fixture references.

----

CLI and vendoring tests
-----------------------

Live under ``crates/ethercat-netcfg-cli/tests/``.

.. test:: expand subcommand prints the build-equivalent module
   :id: TEST_0841
   :status: open
   :verifies: REQ_0832

   ``netcfg expand`` over a fixture prints to stdout a module
   byte-identical to the one the build helper writes to ``OUT_DIR`` for
   the same input.

.. test:: fetch vendors a remote ESI and records a pinned lockfile
   :id: TEST_0842
   :status: open
   :verifies: REQ_0833, REQ_0835

   Against a local HTTP test server, ``netcfg fetch`` downloads a
   referenced ESI into the vendored directory and writes a lockfile entry
   carrying the file's content hash and the device revision. A second
   ``fetch`` with the server returning identical bytes is a no-op; a
   server returning altered bytes updates the recorded hash.

.. test:: Build fetches nothing; unvendored URL is a build error
   :id: TEST_0843
   :status: open
   :verifies: REQ_0834

   With network access denied to the build process, a fixture whose
   device references a web-URL ESI that has no vendored, pinned local
   copy fails the build with a "not vendored" error and makes no network
   request. The same fixture after ``netcfg fetch`` builds cleanly
   offline.

----

Validation and bring-up assertion tests
---------------------------------------

Live under ``crates/ethercat-netcfg-codegen/tests/validation/``.

.. test:: Overlapping slices are a build error unless allowed
   :id: TEST_0844
   :status: open
   :verifies: REQ_0836

   A fixture with two routings overlapping the same bit range in the same
   SubDevice and direction fails the build. Adding ``allow_overlap: true``
   to the read-back channel makes the same fixture build. Confirms the
   default-error / opt-in-allow rule of :need:`ADR_0093`'s sibling
   decision.

.. test:: Out-of-image, zero-length, dangling, and collision faults fail the build
   :id: TEST_0845
   :status: open
   :verifies: REQ_0836, REQ_0835

   A table-driven test feeds one fixture per fault — a slice past the ESI
   process-image size, ``bit_length: 0``, a channel naming a
   non-existent device label, a channel-name collision, an
   override-induced address collision, an ESI hash mismatch against the
   lockfile, and a device carrying both an ESI reference and a
   contradicting inline offset — and asserts each fails the build with a
   distinct, named diagnostic.

.. test:: Unmapped process-image gaps warn but do not fail
   :id: TEST_0846
   :status: open
   :verifies: REQ_0837

   A fixture leaving an unmapped bit range in a device's process image
   builds successfully and emits a non-fatal warning naming the gap.

.. test:: Generated bring-up assertions catch identity, alias, and WKC mismatch
   :id: TEST_0847
   :status: open
   :verifies: REQ_0838

   Driving the generated identity / alias table and ``expected_wkc``
   against ``MockBusDriver``, the test injects a wrong device identity at
   a position, a wrong station alias, and a working counter diverging
   from the expectation, and asserts each drives the connector to the
   ``Degraded`` / ``Down`` health path rather than proceeding to OP.

.. test:: SM-watchdog registers resolve and are emitted for output devices
   :id: TEST_0860
   :status: open
   :verifies: REQ_0844

   Resolution + arithmetic + codegen. An rx-carrying device with no
   override resolves to FTTI/2 (default 100 ms → 500 ticks of 100 µs at
   divider 2498); a per-device ``sm_watchdog_timeout_ms`` override and a
   custom ``ftti_ms`` resolve to their own quantized values; an
   input-only device resolves no watchdog. A value-for-value table pins
   the quantization (``ceil(timeout_us / 100)``, clamp ``1..=u16::MAX``,
   ``0 µs`` → 1 tick) against the connector's documented ``SmWatchdog``
   semantics, including the ceil-over-a-tick case. Codegen over a
   two-device fixture emits ``SubDeviceMap::new(..).with_sm_watchdog(
   SmWatchdog { divider, intervals })`` for the output device and a bare
   ``SubDeviceMap::new(..)`` for the input-only device, asserted against
   the parsed AST.

.. test:: SM-watchdog bound and enable are validated at config time
   :id: TEST_0861
   :status: open
   :verifies: REQ_0845

   The :need:`REQ_0845` matrix. PASS: ESI output SM with the watchdog
   trigger enabled and default FTTI/2; an override below the bound. FAIL:
   an override above FTTI/2; an ESI output SM declaring the watchdog
   trigger disabled; an inline output device with no
   ``sm_watchdog_enabled`` attestation; an inline output device attesting
   ``false``. PASS: an inline output device attesting ``true``. The
   inclusive boundary (override exactly at FTTI/2 on the tick grid)
   passes, proving the comparison is ``<=`` against the quantized value;
   an input-only device is skipped by both checks. Each failure asserts a
   distinct, named ``NetcfgError`` variant carrying the device label.

.. test:: No runtime parsing; generated config is static with no heap
   :id: TEST_0848
   :status: open
   :verifies: REQ_0839

   ``cargo tree`` for a consumer of the generated module lists no YAML
   parser (``serde_yaml`` / ``serde_yml``) in the runtime graph. A
   compile-time check confirms the generated ``PDO_MAP`` is a ``&'static``
   binding usable in a ``const`` context, and a no-allocation test
   (under the bounded allocator) confirms consuming the generated tables
   performs no heap allocation.
