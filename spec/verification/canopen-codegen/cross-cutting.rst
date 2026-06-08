Cross-cutting tests
===================

Cross-cutting reproducibility tests
------------------------------------

Verify the build-time determinism quality goal
(:need:`QG_0014`).

.. test:: Repeated codegen runs produce byte-identical output
   :id: TEST_0680
   :status: open
   :verifies: QG_0014, REQ_0763

   Run ``Builder::build()`` twice on the same input set in
   freshly-prepared ``OUT_DIR`` directories. Compare the two
   ``devices.rs`` files with ``sha256``. Assert identical.

.. test:: Input-file ordering does not affect output
   :id: TEST_0681
   :status: open
   :verifies: QG_0014, REQ_0732, REQ_0733

   Same input set, glob returns files in two different orders
   (force the order via explicit ``Builder::file(path)`` calls).
   The two ``devices.rs`` outputs are byte-identical (catches
   HashMap-iteration-order nondeterminism in dedup or collision-
   handling).

.. test:: Layering integrity check (Cargo.toml audit)
   :id: TEST_0682
   :status: open
   :verifies: QG_0015, REQ_0721, REQ_0740

   CI shell check that walks each toolchain crate's
   ``Cargo.toml`` and asserts the allowed-dependency matrix:

   * ``fieldbus-od-core``: no ``ethercrab``, no ``socketcan``, no
     ``taktora-connector-*``, no ``canopen-eds-rt``.
   * ``canopen-eds``: no ``canopen-eds-codegen``,
     no ``canopen-eds-rt``, no ``socketcan``, no ``ethercrab``.
   * ``canopen-eds-codegen``: no ``canopen-eds-rt``, no transport
     crates.
   * ``canopen-eds-build``, ``canopen-eds-cli``: no
     ``canopen-eds-rt``.
   * ``canopen-eds-verify``: no ``canopen-eds-codegen``, no
     ``canopen-eds-rt``, no transport crates.

   Implemented with ``cargo metadata`` + ``jq``; runs in the
   workspace CI job.

Cross-cutting traceability
--------------------------

.. needtable::
   :types: test
   :filter: id >= "TEST_0600" and id <= "TEST_0699"
   :columns: id, title, status, verifies
   :show_filters:
