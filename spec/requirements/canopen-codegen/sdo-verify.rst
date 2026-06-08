EDS ↔ SDO-dump verification
============================

The CI-friendly cross-check (:need:`BB_0087`): parse an EDS file and a
captured SDO-upload JSON dump from a real node, then diff the two on
identity, declared PDO maps, and PDO communication parameters.

.. feat:: EDS ↔ SDO-dump verification
   :id: FEAT_0068
   :status: open
   :satisfies: FEAT_0060

   A CI-friendly cross-check: parse an EDS file and a captured
   SDO-upload JSON dump from a real node, then diff the two on
   identity, declared PDO maps, and PDO communication parameters.
   Catches the "vendor shipped a buggy EDS" failure class at build
   time rather than during cyclic operation. Offline-only this
   round — live-bus verification is out of scope per
   :need:`REQ_0797`.

.. req:: Verifier ingests EDS plus JSON SDO-dump
   :id: REQ_0780
   :status: open
   :satisfies: FEAT_0068

   ``canopen-eds-verify`` shall expose
   ``fn verify(eds: &str, dump: &SdoDump) -> Result<VerifyReport,
   VerifyError>`` that parses both inputs and compares them on:
   ``Identity`` (vendor / product / revision from OD index
   ``0x1018:01..03``), the declared PDO map index list per
   direction and the entries within each declared mapping, PDO
   communication parameters (transmission type, cob-id, event
   timer), and device-type at OD index ``0x1000``.

.. req:: Diagnostic output names the differing field
   :id: REQ_0781
   :status: open
   :satisfies: FEAT_0068

   When a verification fails, the ``VerifyReport`` shall name each
   differing field with both the EDS-side and dump-side values
   (e.g. ``Identity.product_code: eds=0x60900000 dump=0x60910000``)
   rather than reporting only "mismatch".

.. req:: Verifier reuses the parser
   :id: REQ_0782
   :status: open
   :satisfies: FEAT_0068

   The verifier shall consume the same ``EdsFile`` IR produced by
   :need:`FEAT_0062` and shall not maintain a second parse path.
   JSON SDO-dump decoding lives inside the verifier crate. The
   verifier shall not depend on ``canopen-eds-codegen``,
   ``canopen-eds-rt``, or ``taktora-connector-can``.

.. req:: Verifier exits non-zero on mismatch
   :id: REQ_0783
   :status: open
   :satisfies: FEAT_0068

   When invoked as a binary
   (``canopen-eds-verify <eds> <dump.json>``), the verifier shall
   exit ``0`` on match, ``1`` on any field mismatch, and ``2`` on
   parse or I/O errors. CI gates may then
   ``cargo run -p canopen-eds-verify -- ...`` as a pre-merge check.

.. req:: SDO-dump JSON schema versioned
   :id: REQ_0784
   :status: open
   :satisfies: FEAT_0068

   The SDO-dump file format shall be versioned via a top-level
   ``schema`` field carrying the string
   ``taktora.canopen.sdo-dump.v1``. Unknown schema strings shall be
   rejected with a parse error before any field comparison runs
   (per :need:`ADR_0086`).
