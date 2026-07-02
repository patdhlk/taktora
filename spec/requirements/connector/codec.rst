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

.. req:: Fixed-width binary codec
   :id: REQ_0215
   :status: implemented
   :satisfies: FEAT_0032
   :links: BB_0003, TEST_0195

   The framework shall ship a ``BinaryCodec`` in ``taktora-connector-codec``
   behind an opt-in ``binary`` cargo feature (not default-on), providing a
   fixed-width binary encoding with **selectable endianness** (default
   big-endian, for network / EtherCAT-PDI byte order) and a **constant-length
   contract**: each fixed-width primitive encodes to a constant number of bytes
   independent of value (``u16`` → 2, ``u32`` → 4, …), so a cyclic-fieldbus
   routing slice can use a static ``bit_length`` rather than the
   variable-length-text workarounds a JSON codec forces. Variable-length types
   (``String`` / ``Vec`` / enums) carry no constant-length guarantee and shall be
   documented as such.

.. req:: MessagePack codec
   :id: REQ_0989
   :status: implemented
   :satisfies: FEAT_0032
   :links: BB_0003, TEST_0955

   The framework shall ship a ``MsgPackCodec`` in ``taktora-connector-codec``
   behind an opt-in ``msgpack`` cargo feature (not default-on), backed by
   ``rmp-serde``. It provides a compact binary encoding smaller than JSON
   for the same value. Like the ``JsonCodec`` it makes no constant-length
   guarantee — MessagePack integers are variable-length and structs encode
   as positional arrays, so encoder and decoder must share the Rust type.
   A successful encode into the caller's buffer shall not allocate; buffer
   exhaustion surfaces as ``ConnectorError::PayloadOverflow`` and other
   serializer or decoder faults as ``ConnectorError::Codec``, consistent
   with :need:`REQ_0213` and :need:`REQ_0214`.
