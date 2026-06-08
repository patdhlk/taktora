CLI inspection
==============

A ``cargo`` subcommand (:need:`BB_0065`) so users can inspect what was
generated for a given ESI file without going through ``$OUT_DIR`` /
``cargo expand``.

.. feat:: CLI inspection (cargo subcommand)
   :id: FEAT_0056
   :status: open
   :satisfies: FEAT_0050

   A ``cargo`` subcommand so users can inspect what was generated
   for a given ESI file without going through ``$OUT_DIR`` /
   ``cargo expand``. Adds discoverability with one extra crate, no
   change to the codegen path.

.. req:: cargo esi expand emits one device's generated code
   :id: REQ_0550
   :status: open
   :satisfies: FEAT_0056

   ``ethercat-esi-cli`` shall expose a ``cargo esi expand --device
   <ident>`` subcommand that parses the matching ESI file(s) and
   prints the generated module for that device to stdout, formatted
   per :need:`REQ_0543`.

.. req:: cargo esi list enumerates devices in a glob
   :id: REQ_0551
   :status: open
   :satisfies: FEAT_0056

   ``cargo esi list`` shall accept a glob pattern (defaulting to
   ``esi/*.xml`` when invoked from a crate root) and print the
   ``(ident, vendor_id, product_id, revision)`` tuple for every
   device found.

.. req:: CLI shares the parser and codegen crates
   :id: REQ_0552
   :status: open
   :satisfies: FEAT_0056

   The CLI shall depend on ``ethercat-esi`` and
   ``ethercat-esi-codegen-ethercrab`` as library dependencies. It
   shall not duplicate parse or emit logic. Output produced by the
   CLI for a given input shall be byte-identical to the output
   produced by ``ethercat-esi-build`` for the same input and
   formatter settings (:need:`REQ_0543`).
