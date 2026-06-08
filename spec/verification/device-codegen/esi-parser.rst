Parser unit tests
=================

Per-crate, no I/O beyond test fixtures, parallel-safe. Live under
``crates/ethercat-esi/tests/``.

.. test:: parse() accepts a representative Beckhoff EL3001 ESI
   :id: TEST_0400
   :status: open
   :verifies: REQ_0500, REQ_0504

   Loads a canonical Beckhoff ``EL3001`` ESI XML fixture from
   ``crates/ethercat-esi/tests/fixtures/``, calls
   ``parse(xml)``, and asserts the resulting ``EsiFile`` exposes
   the expected identity (``vendor_id = 0x2``, ``product_id =
   0x0BB93052``), one device with two sync managers, the
   "Standard" and "Compact" PDO alternatives, the expected
   InitCmd count, and a non-empty object dictionary.

.. test:: Parser compiles under no_std + alloc
   :id: TEST_0401
   :status: rejected
   :verifies: REQ_0501

   Compile-only test: a small bin target inside
   ``crates/ethercat-esi/tests/no_std/`` declares ``#![no_std]``,
   uses ``alloc::string::String``, and parses a fixture. The
   target must compile with ``--no-default-features`` against the
   crate; the test passes if compilation succeeds (no runtime
   assertion).

   **Superseded.** The parser now targets ``std`` (see :need:`ADR_0097`);
   :need:`REQ_0501` is rejected, so this compile-only ``no_std`` test no
   longer applies.

.. test:: Parser is independent of ethercrab
   :id: TEST_0402
   :status: open
   :verifies: REQ_0503

   ``cargo tree -p ethercat-esi --no-default-features --target
   <host>`` shall not list ``ethercrab`` anywhere in the
   resolved graph. Implemented as a CI shell check that greps
   the output and fails on match.

.. test:: Vendor-specific elements survive as RawXml
   :id: TEST_0403
   :status: open
   :verifies: REQ_0505

   Fixture file with a fabricated ``<Vendor:UnknownElement
   foo="bar">inner</Vendor:UnknownElement>`` inside a
   ``<Device>``. After parsing, the IR carries one ``RawXml``
   entry with ``name = "Vendor:UnknownElement"``, ``attributes =
   {"foo": "bar"}``, ``inner_text = "inner"``. The parse does
   **not** return an error.

.. test:: Parse errors carry line and column
   :id: TEST_0404
   :status: open
   :verifies: REQ_0506

   A deliberately malformed ESI fixture (unclosed tag at known
   coordinates) parses to ``Err(EsiError::Xml { line, column, ..
   })`` with the expected line and column. Catching this trace
   in build-time output (per :need:`REQ_0506`) is the user
   benefit.

.. test:: Per-SM watchdog-trigger enable decodes from control-byte bit 6
   :id: TEST_0859
   :status: open
   :verifies: REQ_0843

   Unit tests in ``crates/taktora-ethercat-esi/tests/sm_watchdog.rs``
   parse minimal inline ESI documents and assert
   ``SyncManager::watchdog_trigger_enable`` against control-byte bit 6
   (``0x40``). Control byte ``#x64`` (the real WAGO 750-354 outputs SM,
   bit 6 set) decodes to ``true``; ``#x22`` (WAGO mailbox-in SM), ``#x00``
   (WAGO inputs SM), and ``#x24`` (all bit-6-clear) decode to ``false``;
   and an ``<Sm>`` with no ``ControlByte`` attribute decodes to ``false``,
   confirming the parser never fabricates an enabled watchdog from an
   absent control byte.

.. test:: FMMU declarations parse in order with tolerated unknowns
   :id: TEST_0865
   :status: open
   :verifies: REQ_0848

   Unit tests in ``crates/taktora-ethercat-esi/tests/fmmu.rs`` parse
   inline ESI documents and the committed Beckhoff EL3602 fixture,
   asserting declaration order, the ``Inputs`` / ``Outputs`` /
   ``MBoxState`` decodings, attribute-bearing ``<Fmmu>`` elements, and
   that an unrecognised usage string is preserved as
   ``FmmuUsage::Other`` without failing the parse.

.. test:: EEPROM source data decodes; bad hex is a located error
   :id: TEST_0866
   :status: open
   :verifies: REQ_0849

   Unit tests in ``crates/taktora-ethercat-esi/tests/eeprom.rs`` parse
   inline ESI documents and the committed Beckhoff EL1008 / EL3602
   fixtures, asserting ``ByteSize`` / ``ConfigData`` / ``BootStrap``
   decode to the expected bytes (lower- and uppercase hex), malformed
   and odd-length hex payloads raise ``EsiError::Value`` with path
   ``Eeprom.ConfigData``, ``<Category>`` children (including
   self-closing elements) are captured as ``RawXml``, and a typed
   ``<Eeprom>`` no longer appears among the vendor extensions.

.. test:: MDP catalog and slots parse from synthetic and real modular ESI
   :id: TEST_0867
   :status: open
   :verifies: REQ_0850

   Unit tests in ``crates/taktora-ethercat-esi/tests/modules.rs`` parse
   the synthetic ``modular_coupler.xml`` fixture, asserting the
   three-module catalog (idents, names, per-module PDOs), the slot
   constraints (increments, min/max instances, default module idents),
   malformed module-ident error paths, that a modules-only document
   parses with empty devices, and that a non-modular fixture yields no
   modules and no slots. The gated test ``tests/wago_real.rs``
   additionally parses a real WAGO 750-354 coupler ESI when present in
   ``tests/fixtures/vendor/`` (not redistributed; see that directory's
   PROVENANCE.md).
