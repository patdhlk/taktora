Build helper (build.rs glue)
============================

The trivial helper crate (:need:`BB_0064`) so downstream consumers run
codegen with one ``build.rs`` invocation and one ``include!`` line.

.. feat:: Build helper (build.rs glue)
   :id: FEAT_0055
   :status: open
   :satisfies: FEAT_0050

   A trivial helper crate so downstream consumers run codegen with
   one ``build.rs`` invocation and one ``include!`` line.

.. req:: Builder API shape
   :id: REQ_0540
   :status: open
   :satisfies: FEAT_0055

   ``ethercat-esi-build`` shall expose
   ``Builder::new().glob(<pattern>).backend(<backend>).out_file(<name>).build()``
   returning ``Result<(), BuildError>``. The ``backend`` parameter
   shall be generic over ``CodegenBackend`` per :need:`REQ_0510`.

.. req:: Output written to OUT_DIR
   :id: REQ_0541
   :status: open
   :satisfies: FEAT_0055

   The helper shall write the generated module to
   ``$OUT_DIR/<out_file>`` so consumers wire it in with
   ``include!(concat!(env!("OUT_DIR"), "/<out_file>"));``.

.. req:: Cargo rerun-if directives emitted per ESI input
   :id: REQ_0542
   :status: open
   :satisfies: FEAT_0055

   The helper shall print ``cargo:rerun-if-changed=<path>`` for each
   ESI file matched by the glob and for the build script itself, so
   cargo re-runs codegen exactly when an input changes — not on every
   build.

.. req:: Generated output passes through prettyplease
   :id: REQ_0543
   :status: open
   :satisfies: FEAT_0055

   Before writing the output, the helper shall format the
   ``TokenStream`` via ``prettyplease::unparse`` so the file is
   human-readable when diffed or inspected through ``cargo expand``.
