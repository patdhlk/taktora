Cross-cutting tests
===================

Cross-cutting reproducibility tests
------------------------------------

Verify the build-time determinism quality goal
(:need:`QG_0010`).

.. test:: Repeated codegen runs produce byte-identical output
   :id: TEST_0470
   :status: open
   :verifies: QG_0010, REQ_0543

   Run ``Builder::build()`` twice on the same input set in
   freshly-prepared ``OUT_DIR`` directories. Compare the two
   ``devices.rs`` files with ``sha256``. Assert identical.

.. test:: Input-file ordering does not affect output
   :id: TEST_0471
   :status: open
   :verifies: QG_0010, REQ_0512, REQ_0513

   Same input set, glob returns files in two different orders
   (force the order via explicit ``Builder::file(path)``
   calls). The two ``devices.rs`` outputs are byte-identical
   (catches HashMap-iteration-order nondeterminism in dedup or
   collision-handling).

.. test:: Layering integrity check (Cargo.toml audit)
   :id: TEST_0472
   :status: open
   :verifies: QG_0011, REQ_0503, REQ_0520

   CI shell check that walks each toolchain crate's
   ``Cargo.toml`` and asserts the allowed-dependency matrix:

   * ``ethercat-esi``: no ``ethercrab``, no ``proc-macro2``, no
     ``quote``, no codegen crate.
   * ``ethercat-esi-codegen``: no ``ethercrab``.
   * ``ethercat-esi-build``: no ``ethercrab`` (transitively via
     ``ethercat-esi-codegen``-only path).
   * ``ethercat-esi-verify``: no ``ethercrab``.

   Implemented with ``cargo metadata`` + ``jq``; runs in the
   workspace CI job.

Cross-cutting traceability
--------------------------

.. needtable::
   :types: test
   :filter: id >= "TEST_0400" and id <= "TEST_0499"
   :columns: id, title, status, verifies
   :show_filters:
