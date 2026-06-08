EEPROM diff verification
========================

A CI-friendly cross-check (:need:`BB_0066`): parse an ESI XML file and
the matching captured SII EEPROM ``.bin`` dump, then diff the two.

.. feat:: EEPROM diff verification
   :id: FEAT_0057
   :status: open
   :satisfies: FEAT_0050

   A CI-friendly cross-check: parse an ESI XML file and the matching
   captured SII EEPROM ``.bin`` dump from a real device, then diff
   the two on identity, PDO map, and mailbox configuration. Catches
   the "vendor shipped a buggy ESI file" failure class at build time
   rather than during cyclic operation.

.. req:: Verifier ingests ESI XML plus SII binary
   :id: REQ_0560
   :status: open
   :satisfies: FEAT_0057

   ``ethercat-esi-verify`` shall expose
   ``fn verify(xml: &str, sii: &[u8]) -> Result<VerifyReport,
   VerifyError>`` that parses both inputs and compares them on:
   ``Identity`` (vendor / product / revision), the assigned PDO
   index list per direction, and the mailbox bootstrap configuration
   (CoE/EoE/FoE supported sets).

.. req:: Diagnostic output names the differing field
   :id: REQ_0561
   :status: open
   :satisfies: FEAT_0057

   When a verification fails, the ``VerifyReport`` shall name each
   differing field with both the ESI-side and SII-side values
   (e.g. ``Identity.revision: esi=0x00100000 sii=0x00110000``)
   rather than reporting only "mismatch".

.. req:: Verifier reuses the parser
   :id: REQ_0562
   :status: open
   :satisfies: FEAT_0057

   The verifier shall consume the same ``EsiFile`` IR produced by
   :need:`FEAT_0051` and shall not maintain a second parse path. SII
   binary decoding lives inside the verifier crate (no reuse from
   ethercrab — the verifier shall not depend on ethercrab per
   :need:`REQ_0520`).

.. req:: Verifier exits non-zero on mismatch
   :id: REQ_0563
   :status: open
   :satisfies: FEAT_0057

   When invoked as a binary (``ethercat-esi-verify <xml> <sii>``),
   the verifier shall exit ``0`` on match, ``1`` on any field
   mismatch, and ``2`` on parse or I/O errors. CI gates may then
   ``cargo run -p ethercat-esi-verify -- ...`` as a pre-merge
   check.
