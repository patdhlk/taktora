Build-script glue
=================

Build helper requirements for :need:`FEAT_0083` — wiring the parser and
codegen into a consuming crate's ``build.rs``.

.. feat:: Build-script glue
   :id: FEAT_0083
   :status: open
   :satisfies: FEAT_0080

   A ``build.rs`` helper that wires the parser and codegen into a
   consuming crate's build, emitting the generated module into
   ``OUT_DIR``.

.. req:: Generate into OUT_DIR for include
   :id: REQ_0830
   :status: implemented
   :satisfies: FEAT_0083
   :links: BB_0126, TEST_0839

   The build helper shall read the configured ``network.yaml``, run
   parse + codegen, and write one Rust module into ``OUT_DIR`` for the
   consumer to pull in via ``include!(concat!(env!("OUT_DIR"),
   "/network.rs"))``. The generated module shall not be checked into
   version control.

.. req:: Rebuild on config or ESI change
   :id: REQ_0831
   :status: open
   :satisfies: FEAT_0083

   The build helper shall emit ``cargo:rerun-if-changed`` directives for
   the ``network.yaml`` and every vendored ESI file it resolves, so a
   change to topology or a referenced device description triggers
   regeneration.
