Codegen tests
=============

Unit tests of the plane-generic codegen and the CAN backend's token
emission. No build-time codegen here — the generated-code round-trips
live on :doc:`can-backend`.

.. test:: Naming policy and identifier-collision detection
   :id: TEST_0930
   :status: implemented
   :verifies: REQ_0954

   Inline ``#[cfg(test)]`` cases in
   ``crates/taktora-idl-codegen/src/lib.rs`` and
   ``crates/taktora-idl-codegen/src/naming.rs``: assert type / field /
   variant naming (PascalCase, snake_case), Rust-keyword escaping, and
   leading-digit prefixing, and assert that two distinct source names
   normalising to one identifier (type, field, or variant) is rejected as
   a collision rather than silently emitted. Confirms :need:`REQ_0954`.

.. test:: CAN backend emits parseable WireType tokens
   :id: TEST_0931
   :status: implemented
   :verifies: REQ_0955, REQ_0956

   Inline ``#[cfg(test)]`` case in
   ``crates/taktora-idl-codegen-can/src/lib.rs``: parses a sample DBC,
   lowers it to IR plus layout, drives ``generate`` with ``CanBackend``,
   and asserts the emitted ``TokenStream`` parses as valid Rust and
   contains the expected elements (a ``WireType`` impl, the value-table
   enum, snake_case fields, and ``pack_signed`` calls). Confirms the
   ``MessageBackend`` / ``generate`` contract (:need:`REQ_0955`) and the
   CAN emission (:need:`REQ_0956`).
