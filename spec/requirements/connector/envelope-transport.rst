Envelope transport
==================

The on-wire form of every message crossing the plugin↔gateway boundary, and
the iceoryx2 service shape that carries it. This cluster ``:satisfies:``
:need:`FEAT_0030`.

.. feat:: Envelope transport
   :id: FEAT_0031
   :status: open
   :satisfies: FEAT_0030

   The on-wire form of every message crossing the plugin↔gateway boundary
   and the iceoryx2 service shape that carries it. Defines header fields,
   per-channel sizing, and the zero-copy publish path.

.. req:: ConnectorEnvelope is a POD type
   :id: REQ_0200
   :status: open
   :satisfies: FEAT_0031

   The framework shall define ``ConnectorEnvelope`` as a ``#[repr(C)]``
   plain-old-data type that derives ``ZeroCopySend`` (iceoryx2) and
   contains a fixed header (sequence number, timestamp, payload length,
   correlation id, reserved word) followed by an inline payload buffer.

.. req:: Per-channel max payload size
   :id: REQ_0201
   :status: approved
   :satisfies: FEAT_0031

   The framework shall allow each channel to declare its maximum payload
   size at service-creation time, carried in ``ChannelDescriptor``. A
   channel's envelope payload buffer shall be sized to that maximum; no
   universal payload ceiling is imposed across the framework.

.. req:: Sequence number monotonically increasing
   :id: REQ_0202
   :status: implemented
   :satisfies: FEAT_0031
   :links: BB_0010, TEST_0121

   For each (publisher, channel) pair, the framework shall populate
   ``ConnectorEnvelope::sequence_number`` with a strictly monotonically
   increasing ``u64`` so receivers can detect missed envelopes.

.. req:: Timestamp recorded at send
   :id: REQ_0203
   :status: implemented
   :satisfies: FEAT_0031
   :links: BB_0010, TEST_0122

   The framework shall populate ``ConnectorEnvelope::timestamp_ns`` with
   nanoseconds since the UNIX epoch at the moment the envelope is loaned
   for send.

.. req:: Correlation id is a passive carrier
   :id: REQ_0204
   :status: implemented
   :satisfies: FEAT_0031
   :links: BB_0010, TEST_0123

   The framework shall carry the 32-byte ``correlation_id`` field
   end-to-end from sender to receiver without inspecting it. Application
   layers may use this field for request/response matching; the framework
   itself shall not.

.. req:: Zero-copy publish via iceoryx2 loan
   :id: REQ_0205
   :status: implemented
   :satisfies: FEAT_0031
   :links: BB_0002, TEST_0120

   The framework shall publish envelopes via ``Publisher::loan`` such that
   the codec writes the payload directly into shared memory. No envelope
   shall be copied between an intermediate user-side buffer and shared
   memory on the send path.

.. req:: One iceoryx2 service per channel direction
   :id: REQ_0206
   :status: implemented
   :satisfies: FEAT_0031
   :links: BB_0011, TEST_0126

   For each logical channel direction (outbound app→gateway, inbound
   gateway→app), the framework shall create a separate iceoryx2
   publish-subscribe service whose name is derived deterministically from
   ``ChannelDescriptor::name``.
