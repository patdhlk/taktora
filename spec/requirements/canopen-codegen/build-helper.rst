Build helper (build.rs glue)
============================

The trivial helper crate (:need:`BB_0085`) so downstream consumers run
codegen with one ``build.rs`` invocation and one ``include!`` line.

.. feat:: Build helper (build.rs glue)
   :id: FEAT_0066
   :status: open
   :satisfies: FEAT_0060

   A trivial helper crate so downstream consumers run codegen with
   one ``build.rs`` invocation and one ``include!`` line.

.. req:: Builder API shape
   :id: REQ_0760
   :status: open
   :satisfies: FEAT_0066

   ``canopen-eds-build`` shall expose
   ``Builder::new().glob(<pattern>).backend(<backend>).out_file(<name>).build()``
   returning ``Result<(), BuildError>``. The ``backend`` parameter
   shall be generic over ``CodegenBackend`` per :need:`REQ_0730`.

.. req:: Output written to OUT_DIR
   :id: REQ_0761
   :status: open
   :satisfies: FEAT_0066

   The helper shall write the generated module to
   ``$OUT_DIR/<out_file>`` so consumers wire it in with
   ``include!(concat!(env!("OUT_DIR"), "/<out_file>"));``.

.. req:: Cargo rerun-if directives emitted per EDS input
   :id: REQ_0762
   :status: open
   :satisfies: FEAT_0066

   The helper shall print ``cargo:rerun-if-changed=<path>`` for
   each EDS file matched by the glob and for the build script
   itself, so cargo re-runs codegen exactly when an input changes —
   not on every build.

.. req:: Generated output passes through prettyplease
   :id: REQ_0763
   :status: open
   :satisfies: FEAT_0066

   Before writing the output, the helper shall format the
   ``TokenStream`` via ``prettyplease::unparse`` so the file is
   human-readable when diffed or inspected (per :need:`ADR_0076`).

.. req:: Parser warnings surface as cargo warnings
   :id: REQ_0764
   :status: open
   :satisfies: FEAT_0066

   Parser warnings raised under :need:`REQ_0725` shall surface as
   ``cargo:warning=<line>: <kind>`` lines so they appear in cargo
   build output. A strict mode that promotes warnings to errors is
   not provided in this round.
