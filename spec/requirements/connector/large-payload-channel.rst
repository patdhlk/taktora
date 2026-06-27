Large / variable-payload slice channel
======================================

A second delivery mechanism in ``taktora-connector-transport-iox``, additive to
the fixed-``N`` ``ConnectorEnvelope<N>`` POD envelope: a variable-length,
zero-copy channel for bulk one-shot payloads whose size is not known at compile
time. It exists so higher-layer connectors (first consumer:
``taktora-connector-j1939`` ETP, :need:`REQ_0894`) can move large messages —
firmware blobs, camera frames — without inlining a worst-case buffer into every
sample. This cluster ``:satisfies:`` :need:`FEAT_0030`.

.. feat:: Large / variable-payload slice channel
   :id: FEAT_0097
   :status: draft
   :satisfies: FEAT_0030

   A variable-length channel pair backed by an iceoryx2 slice (``[u8]``)
   publish-subscribe service. Where :need:`FEAT_0031`'s
   ``ConnectorEnvelope<N>`` pins payload capacity at compile time, this
   channel sizes each sample to the message at send time and lets the
   shared-memory segment grow on demand, capped by a configurable
   ceiling so the bounded-allocation posture of :need:`FEAT_0040` still
   holds. It is an opt-in bulk path: connectors with statically-known
   payload sizes keep using the POD envelope.

.. req:: Slice-typed variable-length channel
   :id: REQ_0885
   :status: implemented
   :links: BB_0097, TEST_0884
   :satisfies: FEAT_0097
   :github: 121

   ``taktora-connector-transport-iox`` shall provide a variable-length
   channel pair (``SliceChannelWriter`` / ``SliceChannelReader``) backed
   by an iceoryx2 slice (``[u8]``) publish-subscribe service that
   delivers exactly one message per sample, zero-copy.

.. req:: Loans sized at send time
   :id: REQ_0886
   :status: implemented
   :links: BB_0097, TEST_0884
   :satisfies: FEAT_0097
   :github: 121

   The slice channel shall size each published sample to the message
   length at send time via iceoryx2 ``loan_slice`` / ``loan_slice_uninit``,
   rather than a compile-time ``N`` const generic.

.. req:: Segment grows by PowerOfTwo
   :id: REQ_0887
   :status: implemented
   :links: BB_0097, TEST_0885
   :satisfies: FEAT_0097
   :github: 121

   The slice channel shall start from a configurable
   ``initial_max_slice_len`` and grow its shared-memory data segment
   using ``AllocationStrategy::PowerOfTwo`` when a message larger than the
   current segment is loaned.

.. req:: Growth bounded by a configurable ceiling
   :id: REQ_0888
   :status: implemented
   :links: BB_0097, TEST_0885
   :satisfies: FEAT_0097
   :github: 121

   The slice channel shall reject — with a bounded-capacity
   ``ConnectorError`` — any loan whose length exceeds a configurable
   ``max_payload_bytes`` ceiling, so segment growth stays bounded and
   auditable rather than open-ended (upholds the bounded-allocation
   posture of :need:`FEAT_0040`).

.. req:: Single-publisher topology and metadata parity
   :id: REQ_0889
   :status: implemented
   :links: BB_0097, TEST_0884
   :satisfies: FEAT_0097
   :github: 121

   The slice channel shall preserve the framework's single-publisher
   iceoryx2 topology and shall carry the same ``sequence_number`` and
   ``timestamp_ns`` metadata semantics that :need:`REQ_0202` and
   :need:`REQ_0203` define for ``ConnectorEnvelope``, exposed through an
   iceoryx2 user-header rather than an inline POD struct.
