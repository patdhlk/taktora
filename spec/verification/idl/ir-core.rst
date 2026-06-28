IR-core unit tests
==================

Per-crate, no I/O beyond synthetic inputs, parallel-safe. Live under
``crates/taktora-idl-core/tests/``.

.. test:: Boundedness and structural-soundness contract
   :id: TEST_0920
   :status: implemented
   :verifies: REQ_0930, REQ_0931, REQ_0932

   ``crates/taktora-idl-core/tests/boundedness.rs``: constructs synthetic
   modules and asserts the boundedness contract — scalar wire sizes,
   integer-width narrowing, flat and nested struct sizing, bounded string
   and sequence bounds — and the structural checks — type / enum / field
   resolution, rejection of a recursive struct, rejection of duplicate
   names, and service request/response payload validation. Confirms
   ``validate`` and ``max_serialized_len`` behave per :need:`REQ_0930`
   and :need:`REQ_0931`, exercising the policy-free IR surface of
   :need:`REQ_0932`.
