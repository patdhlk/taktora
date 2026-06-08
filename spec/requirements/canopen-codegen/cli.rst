CLI inspection (cargo subcommand)
==================================

The ``cargo`` subcommand (:need:`BB_0086`) so users can inspect what was
generated for a given EDS file without going through ``$OUT_DIR`` or
``cargo expand``.

.. feat:: CLI inspection (cargo subcommand)
   :id: FEAT_0067
   :status: open
   :satisfies: FEAT_0060

   A ``cargo`` subcommand so users can inspect what was generated
   for a given EDS file without going through ``$OUT_DIR`` /
   ``cargo expand``. Adds discoverability with one extra crate, no
   change to the codegen path (per :need:`ADR_0077`).

.. req:: cargo eds expand emits one device's generated code
   :id: REQ_0770
   :status: open
   :satisfies: FEAT_0067

   ``canopen-eds-cli`` shall expose a
   ``cargo eds expand --device <ident>`` subcommand that parses
   the matching EDS file(s) and prints the generated module for
   that device to stdout, formatted per :need:`REQ_0763`.

.. req:: cargo eds list enumerates devices in a glob
   :id: REQ_0771
   :status: open
   :satisfies: FEAT_0067

   ``cargo eds list`` shall accept a glob pattern (defaulting to
   ``eds/*.eds`` when invoked from a crate root) and print the
   ``(ident, vendor_id, product_code, revision)`` tuple for every
   device found.

.. req:: CLI shares the parser and codegen crates
   :id: REQ_0772
   :status: open
   :satisfies: FEAT_0067

   The CLI shall depend on ``canopen-eds`` and
   ``canopen-eds-codegen-taktora`` as library dependencies. It shall
   not duplicate parse or emit logic. Output produced by the CLI
   for a given input shall be byte-identical to the output produced
   by ``canopen-eds-build`` for the same input and formatter
   settings (:need:`REQ_0763`).
