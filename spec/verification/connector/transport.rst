Transport integration tests
===========================

Iceoryx2 services are real; tests run with ``--test-threads=1``; each
test scopes its own ``Node`` name.

.. test:: ChannelWriter → ChannelReader round-trip
   :id: TEST_0120
   :status: implemented
   :verifies: REQ_0200, REQ_0205, REQ_0223

   End-to-end zero-copy round-trip through a real iceoryx2 service:
   ``writer.send(&value)`` followed by ``reader.try_recv()`` yields
   the same value. Verifies that ``Publisher::loan`` is used (no
   intermediate copies) by asserting on a header field set in-place.

.. test:: Sequence-number monotonicity
   :id: TEST_0121
   :status: implemented
   :verifies: REQ_0202

   Sending N envelopes through a single ``ChannelWriter`` and reading
   them on the corresponding ``ChannelReader`` asserts strictly
   increasing ``sequence_number`` values starting at zero.

.. test:: Timestamp populated at send
   :id: TEST_0122
   :status: implemented
   :verifies: REQ_0203

   Captures wall-clock time before and after ``writer.send``; the
   received envelope's ``timestamp_ns`` falls within the bracket.

.. test:: Correlation ID round-trip
   :id: TEST_0123
   :status: implemented
   :verifies: REQ_0204

   ``writer.send_with_correlation(&value, id)`` followed by
   ``reader.try_recv()`` yields a header whose ``correlation_id``
   bytes equal ``id``. Confirms the framework does not interpret the
   field — random bytes round-trip unchanged.

.. test:: Per-channel size — 4 KB, 64 KB, 1 MB
   :id: TEST_0124
   :status: open
   :verifies: REQ_0201, BB_0010

   Three round-trip tests with channels parameterised at distinct
   ``N`` (4 096, 65 536, 1 048 576). All three succeed; iceoryx2
   services have non-overlapping pool sizes per channel.

.. test:: Payload-overflow rejection
   :id: TEST_0125
   :status: implemented
   :verifies: REQ_0201

   ``writer.send(&value)`` for a value whose encoded form exceeds
   the channel's ``N`` returns
   ``ConnectorError::PayloadOverflow { actual, max }`` and emits no
   envelope on the wire.

.. test:: Service naming derived from descriptor
   :id: TEST_0126
   :status: implemented
   :verifies: REQ_0206, BB_0011

   Two ``ChannelDescriptor`` values with identical ``name`` produce
   identical iceoryx2 service names; differing ``name`` values
   produce different service names. Names follow the convention
   documented in :need:`BB_0011`.
