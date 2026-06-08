Codec tests
===========

.. test:: JsonCodec round-trip property test
   :id: TEST_0110
   :status: open
   :verifies: REQ_0210, REQ_0212

   ``proptest``-driven round-trip for a representative struct:
   ``encode(value, &mut buf)`` followed by ``decode(&buf[..len])``
   yields a value equal to the original under every shrunken input.
   Runs against ``JsonCodec``; will be parameterised over
   ``MsgPackCodec`` and ``ProtoCodec`` once those land.

.. test:: Codec encode error on undersized buffer
   :id: TEST_0111
   :status: open
   :verifies: REQ_0213

   Encoding a value larger than the provided buffer returns
   ``ConnectorError::PayloadOverflow { actual, max }`` so the
   buffer-exhaustion path is distinguishable from genuine serializer
   faults at the codec layer. Other serializer failures (NaN
   floats with strict configuration, non-string map keys, etc.)
   surface as ``ConnectorError::Codec`` carrying the codec's static
   ``format_name()`` and the underlying serializer error chain.
   Routing buffer-overflow to ``PayloadOverflow`` keeps the codec
   layer consistent with :need:`REQ_0323` and :need:`TEST_0125` —
   buffer exhaustion is always the same variant regardless of which
   layer detects it.

.. test:: Codec decode error propagation
   :id: TEST_0112
   :status: open
   :verifies: REQ_0214

   Receiving a payload that fails ``decode<T>`` (e.g. truncated JSON,
   wrong shape) surfaces as ``ConnectorError::Codec`` from
   ``ChannelReader::try_recv`` rather than silently dropping the
   envelope.
