Build-script tests
==================

Integration tests of the ``build.rs`` glue (:need:`FEAT_0083`). Live
under ``crates/ethercat-netcfg-build/tests/``.

.. test:: Build helper generates into OUT_DIR and the module compiles
   :id: TEST_0839
   :status: implemented
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
