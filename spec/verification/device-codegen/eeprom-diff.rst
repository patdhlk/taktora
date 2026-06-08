EEPROM verifier tests
=====================

Live under ``crates/ethercat-esi-verify/tests/``.

.. test:: Verifier passes on matching ESI + SII pair
   :id: TEST_0460
   :status: open
   :verifies: REQ_0560

   Use the captured pair from
   ``crates/ethercat-eeprom-dump/dumps/EL3001/`` (ESI XML +
   matching SII ``.bin``), call
   ``verify(xml, sii)``, assert ``Ok(VerifyReport {
   matched: true, .. })``.

.. test:: Verifier reports the differing field
   :id: TEST_0461
   :status: open
   :verifies: REQ_0561

   Synthetic mismatched pair: parse a real ESI but flip one bit
   in a captured SII to alter the revision field. The returned
   ``VerifyReport`` shall contain at least one ``Difference``
   entry whose ``field`` is exactly ``"Identity.revision"`` and
   whose ``esi`` / ``sii`` values match the originals.

.. test:: Verifier reuses ethercat-esi parser
   :id: TEST_0462
   :status: open
   :verifies: REQ_0562

   White-box: ``cargo tree -p ethercat-esi-verify`` lists
   ``ethercat-esi`` as a direct dependency and does **not** list
   ``ethercrab`` anywhere in the graph (re-affirming
   :need:`TEST_0423` from the verifier side).

.. test:: Verifier exit codes follow the documented matrix
   :id: TEST_0463
   :status: open
   :verifies: REQ_0563

   Spawn the verifier binary three times: matching pair (expect
   exit 0), mismatched pair (expect 1), unreadable SII path
   (expect 2). Asserted via ``Command::status().code()``.
