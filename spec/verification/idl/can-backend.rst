CAN backend / generated-code tests
==================================

End-to-end tests that compile real generated ``WireType`` code. The
``crates/taktora-idl-codegen-can-tests`` harness (``publish = false``)
runs the ``.dbc`` → IR → codegen → format pipeline in its ``build.rs``,
compiles the generated module, and round-trips it. These tests double as
the verification of the ``taktora-idl-wire`` primitives, which have no
unit tests of their own.

.. test:: Generated round-trip, DLC sizing, little-endian placement
   :id: TEST_0932
   :status: implemented
   :verifies: REQ_0949, REQ_0950, REQ_0951, REQ_0956, REQ_0957

   ``crates/taktora-idl-codegen-can-tests/src/lib.rs``: with the module
   generated at build time, asserts ``MAX_SERIALIZED_LEN`` equals the
   frame DLC, that an ``encode``/``decode`` cycle preserves field values,
   and that a 16-bit little-endian signal packs LSB-first at the declared
   bit offset (golden byte placement). Confirms the ``WireType`` surface
   (:need:`REQ_0949`), the allocation-free path (:need:`REQ_0950`), the
   little-endian primitive (:need:`REQ_0951`), the CAN emission
   (:need:`REQ_0956`), and the build-time generation pipeline
   (:need:`REQ_0957`).

.. test:: Enum and signed-enum round-trip; unknown value rejected
   :id: TEST_0933
   :status: implemented
   :verifies: REQ_0951, REQ_0956

   Asserts a value-table enum field encodes and decodes, that an
   out-of-range discriminant decodes to ``WireError::UnknownEnumValue``,
   and that a signed enum variant (e.g. ``-1`` on a 4-bit signed signal)
   round-trips through two's-complement encode and sign-extending decode.
   Confirms signed/unsigned primitives (:need:`REQ_0951`) and the
   backend's enum emission (:need:`REQ_0956`).

.. test:: Wire error path — buffer and value bounds rejected
   :id: TEST_0934
   :status: implemented
   :verifies: REQ_0949, REQ_0951

   Asserts that ``encode``/``decode`` against an undersized buffer
   returns ``WireError::BufferTooSmall``, and that a field value that
   does not fit its declared bit width returns
   ``WireError::ValueOutOfRange``. Confirms the closed deterministic
   error set of the ``WireType`` surface (:need:`REQ_0949`) and the
   primitives (:need:`REQ_0951`).

.. test:: Multiplexed signals flatten into one struct
   :id: TEST_0935
   :status: implemented
   :verifies: REQ_0953

   Asserts that a multiplexor plus a multiplexed signal in the fixture
   DBC lower into plain fields of one generated struct (the slice-1
   flattening behaviour) and round-trip. Confirms the lowering rule in
   :need:`REQ_0953`.
