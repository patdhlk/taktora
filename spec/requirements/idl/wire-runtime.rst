Wire runtime
============

The ``serde``-free, ``no_std`` runtime half (:need:`BB_0112`): the
``WireType`` trait the generated code implements and the CAN signal
bit-packing primitives it calls. This is the only layer of the toolchain
that links into a runtime consumer, so it is kept small enough to keep
the (de)serialization path auditable.

.. feat:: serde-free no_std wire runtime
   :id: FEAT_0112
   :status: implemented
   :satisfies: FEAT_0110

   A new crate ``taktora-idl-wire`` carrying the ``WireType`` trait and
   the bit-packing primitives the generated (de)serializers call. The
   split exists so the safety-relevant (de)serialization path is
   ``no_std``, allocation-free, and free of ``serde``/reflection while
   the policy-heavy code emission stays host-side.

.. req:: WireType trait surface
   :id: REQ_0933
   :status: implemented
   :satisfies: FEAT_0112
   :links: BB_0112, TEST_0924, TEST_0926

   ``taktora-idl-wire`` shall define a ``WireType`` trait carrying a
   ``const MAX_SERIALIZED_LEN: usize`` upper bound, an
   ``encode(&self, buf: &mut [u8]) -> Result<usize, WireError>``, and a
   ``decode(buf: &[u8]) -> Result<Self, WireError>``. ``MAX_SERIALIZED_LEN``
   shall be a true upper bound on the bytes ``encode`` writes, so a
   caller can size a buffer from the type alone.

.. req:: no_std, allocation-free, serde-free path
   :id: REQ_0934
   :status: implemented
   :satisfies: FEAT_0112
   :links: BB_0112, TEST_0924

   ``taktora-idl-wire`` shall be ``#![no_std]``, perform no heap
   allocation on the encode/decode path, and depend on no external
   crate — in particular not ``serde`` and not a reflection facility.
   Generated ``WireType`` implementations shall call only the primitives
   in this crate. The intent is a (de)serialization path small enough to
   remain Kani/Miri-tractable.

.. req:: CAN signal bit-packing primitives
   :id: REQ_0935
   :status: implemented
   :satisfies: FEAT_0112
   :links: BB_0112, TEST_0924, TEST_0925, TEST_0926

   ``taktora-idl-wire`` shall provide signed and unsigned pack/unpack
   primitives over a byte frame addressed by start bit and bit length,
   handling both DBC bit-numbering conventions — Intel little-endian
   (``@1``) and Motorola big-endian (``@0``) — through one shared
   bit-mapping so encode and decode cannot drift. Errors shall be a
   closed, deterministic set: buffer too small, signal out of frame
   bounds, invalid bit length, value out of range, and unknown enum
   value.
