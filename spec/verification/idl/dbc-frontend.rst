DBC frontend tests
==================

Per-crate, no I/O beyond synthetic ``.dbc`` text, parallel-safe. Live
under ``crates/taktora-idl-dbc/tests/``.

.. test:: DBC parse and lower round-trip
   :id: TEST_0921
   :status: implemented
   :verifies: REQ_0936, REQ_0937

   ``crates/taktora-idl-dbc/tests/roundtrip.rs``: parses fixture DBC text
   and asserts the parsed structure (version, nodes, messages, signals,
   multiplexer roles, value tables), then lowers it and asserts the IR
   (messages → bounded structs, scalar type inference from bit widths,
   value tables → enums) and the ``DbcLayout`` sidecar (per-signal start
   bit / bit length / byte order, per-frame CAN id and extended-id flag).
   Confirms ``parse`` (:need:`REQ_0936`) and ``lower`` (:need:`REQ_0937`).
