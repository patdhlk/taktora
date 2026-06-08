CLI tests
=========

Live under ``crates/ethercat-esi-cli/tests/``.

.. test:: cargo esi expand emits a single device's code
   :id: TEST_0450
   :status: open
   :verifies: REQ_0550

   Spawn the CLI as ``cargo esi expand --device EL3001 --glob
   <fixtures>/*.xml``, capture stdout, assert the output is
   non-empty, parses as Rust, and contains exactly one ``pub
   struct EL3001`` definition.

.. test:: cargo esi list enumerates devices
   :id: TEST_0451
   :status: open
   :verifies: REQ_0551

   Run ``cargo esi list --glob <fixtures>/*.xml`` over a
   3-device fixture set, assert stdout contains 3 lines each
   matching the ``<ident>\t<vendor_id>\t<product_id>\t<revision>``
   format.

.. test:: CLI output matches build helper output byte-for-byte
   :id: TEST_0452
   :status: open
   :verifies: REQ_0552

   For one fixed device, capture the CLI's ``expand`` output and
   the build helper's per-device slice of ``$OUT_DIR/devices.rs``.
   Assert the two byte-strings are identical (catches divergent
   code paths between CLI and build).
