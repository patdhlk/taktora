CLI tests
=========

Live under ``crates/canopen-eds-cli/tests/``.

.. test:: cargo eds expand emits a single device's code
   :id: TEST_0660
   :status: open
   :verifies: REQ_0770

   Spawn the CLI as ``cargo eds expand --device EPOS4 --glob
   <fixtures>/*.eds``, capture stdout, assert the output is
   non-empty, parses as Rust, and contains exactly one ``pub
   struct EPOS4_REV0140`` (or equivalent rev-suffixed) definition.

.. test:: cargo eds list enumerates devices
   :id: TEST_0661
   :status: open
   :verifies: REQ_0771

   Run ``cargo eds list --glob <fixtures>/*.eds`` over a
   3-device fixture set, assert stdout contains 3 lines each
   matching the ``<ident>\t<vendor_id>\t<product_code>\t<revision>``
   format.

.. test:: CLI output matches build helper output byte-for-byte
   :id: TEST_0662
   :status: open
   :verifies: REQ_0772

   For one fixed device, capture the CLI's ``expand`` output and
   the build helper's per-device slice of ``$OUT_DIR/devices.rs``.
   Assert the two byte-strings are identical (catches divergent
   code paths between CLI and build).
