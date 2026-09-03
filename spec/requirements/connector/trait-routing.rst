Connector trait and routing
===========================

The plugin-side public API and its typed routing contract. This cluster
``:satisfies:`` :need:`FEAT_0030`.

.. feat:: Connector trait and routing
   :id: FEAT_0033
   :status: implemented
   :satisfies: FEAT_0030

   The plugin-side public API: a ``Connector`` trait every connector
   implements, parameterised on a typed routing struct so plugin code is
   compile-time-checked against the protocol it targets.

.. req:: Connector trait
   :id: REQ_0220
   :status: implemented
   :satisfies: FEAT_0033
   :links: BB_0001, IMPL_0040, TEST_0301, TEST_0957

   The framework shall define a ``Connector`` trait with associated types
   ``Routing: Routing`` and ``Codec: PayloadCodec``, plus methods
   ``name``, ``health``, ``subscribe_health``, ``create_writer<T>``, and
   ``create_reader<T>``.

.. req:: ChannelDescriptor carries typed routing
   :id: REQ_0221
   :status: implemented
   :satisfies: FEAT_0033
   :links: BB_0001, TEST_0103

   ``ChannelDescriptor<R: Routing>`` shall carry a logical channel name,
   the per-channel max payload size, and a typed routing struct ``R``
   declared by the connector crate.

.. req:: Routing is a marker trait with bounds
   :id: REQ_0222
   :status: implemented
   :satisfies: FEAT_0033
   :links: BB_0001, IMPL_0010, TEST_0958

   The ``Routing`` trait shall require ``Clone + Send + Sync + Debug +
   'static`` and shall add no methods of its own.

.. req:: create_writer / create_reader return concrete handles
   :id: REQ_0223
   :status: implemented
   :satisfies: FEAT_0033
   :links: IMPL_0040, TEST_0120

   ``Connector::create_writer<T>`` and ``Connector::create_reader<T>``
   shall return concrete generic types ``ChannelWriter<T, C, N>`` and
   ``ChannelReader<T, C, N>``, not boxed trait objects.

.. req:: Connector ships its own routing struct
   :id: REQ_0224
   :status: implemented
   :satisfies: FEAT_0033
   :links: BB_0020, BB_0041, TEST_0300, TEST_0958

   Each connector crate (``taktora-connector-mqtt``, future
   ``taktora-connector-opcua``, etc.) shall define its own routing struct
   (``MqttRouting``, ``OpcUaRouting``, ...) implementing the ``Routing``
   marker trait, exposing protocol-specific fields.
