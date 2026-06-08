Build helper tests
==================

Live under ``crates/ethercat-esi-build/tests/``.

.. test:: Builder writes a parseable Rust file to OUT_DIR
   :id: TEST_0440
   :status: open
   :verifies: REQ_0540, REQ_0541

   Test crate driven by a fixture ESI set runs
   ``Builder::new().glob(...).backend(default).out_file(
   "devices.rs").build()`` in a ``tempfile``-backed ``OUT_DIR``.
   Asserts the file exists, is non-empty, and parses with
   ``syn::parse_file`` (catches malformed token streams).

.. test:: cargo rerun-if-changed emitted per ESI input
   :id: TEST_0441
   :status: open
   :verifies: REQ_0542

   Capture ``Builder::build()``'s stdout output (the build
   helper prints to stdout per cargo conventions). For an
   N-file glob, assert exactly N + 1 ``cargo:rerun-if-changed=``
   lines are present (one per ESI file + one for the build
   script itself).

.. test:: Output passes prettyplease formatting
   :id: TEST_0442
   :status: open
   :verifies: REQ_0543

   The generated ``devices.rs`` file is line-wrapped (no line
   exceeds 100 chars without justification), and re-running
   ``prettyplease::unparse`` on the file produces a
   byte-identical output (idempotent formatter pass).
