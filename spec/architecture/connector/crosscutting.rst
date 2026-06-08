Crosscutting concepts
=====================

arc42 §8.

These concepts cut across building blocks and runtime scenarios.

.. architecture:: Codec — compile-time generic
   :id: ARCH_0030
   :status: open
   :refines: ADR_0005, BB_0003

   Every connector instance is parameterised on its ``PayloadCodec``.
   Concrete connector types are
   ``MqttConnector<JsonCodec>``,
   ``MqttConnector<MsgPackCodec>`` (when feature-enabled), etc. The
   codec is invoked inside ``Publisher::loan`` so encoded bytes land
   directly in shared memory; on the receive side, ``decode`` runs
   over the borrowed payload slice. There is no intermediate
   serialised buffer.

.. architecture:: Error handling — single error type, explicit origins
   :id: ARCH_0031
   :status: open
   :refines: REQ_0213, REQ_0214

   ``ConnectorError`` is the framework's single error type. Each
   variant has exactly one origin point in the framework; routing of
   variants to user-visible vs. observable surfaces is explicit:

   .. list-table::
      :header-rows: 1
      :widths: 18 27 27 28

      * - Class
        - Originates in
        - Propagates as
        - Surfaces to user as
      * - ``Transport``
        - ``taktora-connector-transport-iox``
        - ``Result`` from ``send`` / ``try_recv``
        - ``Err`` from the call
      * - ``Codec``
        - ``taktora-connector-codec``
        - ``Result`` from ``encode`` / ``decode``
        - ``Err`` from ``send`` (encode) or ``try_recv`` (decode)
      * - ``Routing``
        - gateway, on inbound topic miss
        - ``HealthEvent::RoutingError``
        - observable; gateway never aborts
      * - ``PayloadOverflow``
        - ``ChannelWriter::send`` pre-loan check
        - ``Err`` from ``send``
        - typed; user resizes channel or splits payload
      * - ``Stack``
        - tokio task in gateway
        - ``HealthEvent::StackError`` + ``Down``; triggers reconnect
        - observable; recovers via ``ReconnectPolicy``
      * - ``BackPressure``
        - bridge ``try_send`` failure
        - ``Err`` from ``send`` + ``Degraded``
        - typed; caller retries or drops
      * - ``Down``
        - ``ChannelWriter::send`` pre-check
        - ``Err`` from ``send``
        - typed; caller decides drop vs. retry
      * - ``Shutdown``
        - host shutdown signal
        - ``Err`` from any in-flight op
        - unique variant — caller treats as graceful end

   No silent failures: every error class is either returned to the
   user or emitted as a ``HealthEvent``.

.. architecture:: Observability — Observer + ExecutionMonitor adapter
   :id: ARCH_0032
   :status: open
   :refines: REQ_0273, BB_0005

   The gateway is a taktora-executor consumer (:need:`ADR_0007`), so
   ``Observer::on_app_*`` and ``ExecutionMonitor::pre_execute`` /
   ``post_execute`` hooks already cover the gateway items.
   ``HealthEvent`` arrives on a dedicated taktora-executor
   ``Channel<HealthEvent>`` exposed by ``Connector::subscribe_health``.
   Behind a ``tracing`` cargo feature, ``taktora-connector-host``
   provides an adapter that maps both into ``tracing`` span events
   so a single ``RUST_LOG=...`` config controls the full stack.

.. architecture:: Back-pressure — explicit at every bounded buffer
   :id: ARCH_0033
   :status: open
   :refines: REQ_0260, REQ_0261

   Four bounded buffers participate; saturation surfaces explicitly at
   each. The framework never silently drops outbound user messages;
   inbound is protocol-bounded — drops are reported via
   ``HealthEvent::DroppedInbound`` rather than pretended-prevented.

   .. mermaid::

      flowchart LR
        U[user code] -->|send| W[ChannelWriter]
        W -->|loan/publish| SHM["iceoryx2 SHM<br/>(bounded queue)"]
        SHM -->|wakes| GI[GatewayItem]
        GI -->|try_send| BR1["Tokio bridge OUT<br/>(bounded mpsc)"]
        BR1 --> TT[Tokio task]
        TT -->|publish| B[Broker]
        B -->|publish| TT
        TT -->|send| BR2["Tokio bridge IN<br/>(bounded crossbeam)"]
        BR2 -->|wakes| GI2[InboundGatewayItem]
        GI2 -->|loan/publish| SHM2["iceoryx2 SHM<br/>(bounded queue)"]
        SHM2 --> R[ChannelReader]

----

Cross-cutting traceability
--------------------------

.. needtable::
   :types: building-block
   :columns: id, title, status, implements
   :show_filters:

.. needtable::
   :types: architecture
   :columns: id, title, status, refines
   :show_filters:
