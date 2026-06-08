Verifier tests
==============

Live under ``crates/canopen-eds-verify/tests/``.

.. test:: Verifier passes on matching EDS + dump pair
   :id: TEST_0670
   :status: open
   :verifies: REQ_0780

   Use the captured pair from
   ``crates/canopen-eds-verify/tests/fixtures/EPOS4/`` (EDS file
   + matching ``epos4.dump.json``), call ``verify(eds, dump)``,
   assert ``Ok(VerifyReport { matched: true, .. })``.

.. test:: Verifier reports the differing field
   :id: TEST_0671
   :status: open
   :verifies: REQ_0781

   Synthetic mismatched pair: real EDS, mutated JSON dump
   altering the ``0x1018:02`` product_code field. The returned
   ``VerifyReport`` shall contain at least one ``Difference``
   entry whose ``field`` is exactly ``"Identity.product_code"``
   and whose ``eds`` / ``dump`` values match the originals.

.. test:: Verifier reuses canopen-eds parser
   :id: TEST_0672
   :status: open
   :verifies: REQ_0782

   White-box: ``cargo tree -p canopen-eds-verify`` lists
   ``canopen-eds`` as a direct dependency and does **not** list
   ``canopen-eds-codegen``, ``canopen-eds-rt``,
   ``taktora-connector-can``, or ``socketcan`` anywhere in the
   graph.

.. test:: Verifier exit codes follow the documented matrix
   :id: TEST_0673
   :status: open
   :verifies: REQ_0783

   Spawn the verifier binary three times: matching pair (expect
   exit 0), mismatched pair (expect 1), unreadable JSON path
   (expect 2). Asserted via ``Command::status().code()``.

.. test:: Verifier rejects unknown schema version
   :id: TEST_0674
   :status: open
   :verifies: REQ_0784

   Fixture JSON dump with ``"schema": "taktora.canopen.sdo-dump.v2"``.
   The verifier shall reject the input with a parse error (exit
   code ``2``) before any field comparison runs.
