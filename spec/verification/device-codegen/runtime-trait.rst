Runtime trait surface tests
===========================

Live under ``crates/ethercat-esi-rt/tests/``.

.. test:: EsiDevice trait shape compiles for a hand-written device
   :id: TEST_0430
   :status: open
   :verifies: REQ_0530

   Hand-written test impl of ``EsiDevice`` for a minimal
   ``MockDevice`` validates the trait surface compiles
   end-to-end. Asserts ``IDENTITY``, ``input_len``,
   ``output_len`` return the expected values, and that
   ``decode_inputs`` / ``encode_outputs`` round-trip a synthetic
   ``BitSlice<u8, Lsb0>``.

.. test:: EsiConfigurable async trait shape compiles
   :id: TEST_0431
   :status: open
   :verifies: REQ_0531

   Compile-only test: a mock device implements
   ``EsiConfigurable`` with ``type Assignment = MockAssignment``
   and an ``async fn configure`` body. The test passes if
   compilation succeeds; the async signature shape catches the
   trait-method-async constraint.

.. test:: ethercat-esi-rt is the trait home, not taktora-internal
   :id: TEST_0432
   :status: open
   :verifies: REQ_0532

   CI shell check: ``rg "trait EsiDevice"`` across the workspace
   matches exactly one source location, inside
   ``crates/ethercat-esi-rt/src/``. Same check for
   ``trait EsiConfigurable``.
