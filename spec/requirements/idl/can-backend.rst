CAN/DBC backend
===============

The one concrete backend this round (:need:`BB_0115`): given the DBC
layout sidecar, it emits a ``WireType`` implementation per message that
bit-packs each field at the start bit, length, and byte order the DBC
declared, calling only the ``taktora-idl-wire`` primitives. A build-time
verification harness (:need:`BB_0116`) proves the generated code compiles
and round-trips.

.. feat:: CAN/DBC backend
   :id: FEAT_0115
   :status: implemented
   :satisfies: FEAT_0110

   A new crate ``taktora-idl-codegen-can`` implementing ``MessageBackend``
   for CAN frames, plus the ``taktora-idl-codegen-can-tests``
   verification harness that exercises the full
   ``.dbc`` → IR → codegen → generated-code pipeline.

.. req:: CAN backend emits WireType impls via the wire runtime
   :id: REQ_0956
   :status: implemented
   :satisfies: FEAT_0115
   :links: BB_0115, TEST_0931, TEST_0932, TEST_0933

   ``taktora-idl-codegen-can`` shall implement ``MessageBackend`` so that,
   given a ``DbcLayout`` sidecar, it emits one ``WireType`` implementation
   per message struct whose ``encode``/``decode`` bit-pack each field at
   the DBC-declared start bit, length, and byte order, calling only the
   ``taktora-idl-wire`` primitives (:need:`REQ_0951`) and never ``serde``
   (:need:`REQ_0950`). Naming and field classification arrive already
   resolved from :need:`REQ_0955`; the backend adds only the wire format.
   Integer and enum fields are emitted this round; ``bool`` and float
   fields are rejected (see :doc:`anti-goals`).

.. req:: Build-time generation proof-of-life
   :id: REQ_0957
   :status: implemented
   :satisfies: FEAT_0115
   :links: BB_0116, TEST_0932

   The ``taktora-idl-codegen-can-tests`` crate shall generate
   ``WireType`` code from a fixture ``.dbc`` in its ``build.rs`` — parse,
   lower, generate, parse the tokens as Rust, format with
   ``prettyplease``, and write to ``OUT_DIR`` — and compile the result
   into the crate so the round-trip tests exercise real generated code.
   A failure at any pipeline stage shall fail the build.
