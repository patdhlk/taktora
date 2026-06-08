Build helper tests
==================

Live under ``crates/canopen-eds-build/tests/``.

.. test:: Builder writes a parseable Rust file to OUT_DIR
   :id: TEST_0650
   :status: open
   :verifies: REQ_0760, REQ_0761

   Test crate driven by a fixture EDS set runs
   ``Builder::new().glob(...).backend(default).out_file(
   "devices.rs").build()`` in a ``tempfile``-backed ``OUT_DIR``.
   Asserts the file exists, is non-empty, and parses with
   ``syn::parse_file`` (catches malformed token streams).

.. test:: cargo rerun-if-changed emitted per EDS input
   :id: TEST_0651
   :status: open
   :verifies: REQ_0762

   Capture ``Builder::build()``'s stdout. For an N-file glob,
   assert exactly N + 1 ``cargo:rerun-if-changed=`` lines are
   present (one per EDS file + one for the build script itself).

.. test:: Output passes prettyplease formatting
   :id: TEST_0652
   :status: open
   :verifies: REQ_0763

   The generated ``devices.rs`` file is line-wrapped, and
   re-running ``prettyplease::unparse`` on the file produces a
   byte-identical output (idempotent formatter pass).

.. test:: Parser warnings surface as cargo:warning lines
   :id: TEST_0653
   :status: open
   :verifies: REQ_0764

   Drive the builder against a fixture set including the
   liberal-quirk EDS from :need:`TEST_0615`. Capture stdout, assert
   that one ``cargo:warning=`` line appears per quirk warning
   raised by the parser, and that the build still exits ``Ok``.
