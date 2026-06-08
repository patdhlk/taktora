Codec abstraction
=================

How typed values become payload bytes, and back. This cluster
``:satisfies:`` :need:`FEAT_0030`.

.. feat:: Codec abstraction
   :id: FEAT_0032
   :status: open
   :satisfies: FEAT_0030

   How typed values become payload bytes, and back. Codec selection is a
   compile-time decision via a generic parameter on the connector type;
   no runtime codec dispatch.

.. req:: PayloadCodec trait
   :id: REQ_0210
   :status: implemented
   :satisfies: FEAT_0032
   :links: BB_0003, TEST_0110

   The framework shall define a ``PayloadCodec`` trait carrying
   ``format_name()``, ``encode<T: Serialize>(value, &mut [u8]) -> Result<usize>``,
   and ``decode<T: DeserializeOwned>(&[u8]) -> Result<T>``.

.. req:: Codec is a generic parameter on connectors
   :id: REQ_0211
   :status: open
   :satisfies: FEAT_0032

   Each ``Connector`` implementation shall expose its codec as a generic
   parameter (``MqttConnector<C: PayloadCodec>``), monomorphised at
   compile time. The framework shall not provide runtime codec dispatch
   or ``erased_serde``-style indirection.

.. req:: JsonCodec is the default codec
   :id: REQ_0212
   :status: implemented
   :satisfies: FEAT_0032
   :links: BB_0003, TEST_0110

   The framework shall ship a ``JsonCodec`` implementation in
   ``taktora-connector-codec`` behind a default-on ``json`` cargo feature.

.. req:: Codec encode error variant
   :id: REQ_0213
   :status: open
   :satisfies: FEAT_0032

   When ``PayloadCodec::encode`` fails (buffer too small, serializer error),
   ``ChannelWriter::send`` shall return ``ConnectorError::Codec`` carrying
   the codec's ``format_name()`` and the underlying source error.

.. req:: Codec decode error variant
   :id: REQ_0214
   :status: open
   :satisfies: FEAT_0032

   When ``PayloadCodec::decode`` fails on a received envelope,
   ``ChannelReader::try_recv`` shall return ``ConnectorError::Codec`` and
   shall not silently drop the envelope.
