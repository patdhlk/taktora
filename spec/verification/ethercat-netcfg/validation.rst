Validation and bring-up assertion tests
========================================

Test cases for validation and bring-up assertions (:need:`FEAT_0085`).
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
