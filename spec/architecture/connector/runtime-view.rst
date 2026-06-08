Runtime view
============

arc42 §6.

Four scenarios cover the connector framework's externally-observable
behaviour. Each ``:refines:`` the requirements that govern its
behaviour and the building blocks that implement it.

.. architecture:: Send path (app → broker)
   :id: ARCH_0010
   :status: open
   :refines: REQ_0205, BB_0021, BB_0022

   The send path is fully zero-copy on the sender's side: the codec
   writes directly into shared memory via ``Publisher::loan``.

   .. mermaid::

      sequenceDiagram
        autonumber
        participant U as user code
        participant W as ChannelWriter
        participant P as Publisher (taktora-executor)
        participant SHM as iceoryx2 SHM
        participant S as Subscriber (gateway)
        participant GI as OutboundGatewayItem
        participant BR as Tokio bridge
        participant MQ as rumqttc::AsyncClient
        participant B as Broker

        U->>W: writer.send(&value)
        W->>P: publisher.loan(|slot| codec.encode(value, slot.payload))
        P->>SHM: zero-copy publish + notify
        SHM-->>S: WaitSet wakes
        S->>GI: ExecutableItem::execute()
        GI->>BR: bridge_tx.try_send(payload, routing)
        BR-->>MQ: tokio task drains bridge
        MQ->>B: client.publish(topic, qos, retained, payload)
        B-->>MQ: PUBACK (QoS 1)

.. architecture:: Receive path (broker → app)
   :id: ARCH_0011
   :status: open
   :refines: REQ_0205, REQ_0254, BB_0021, BB_0022

   The receive path mirrors the send path. The gateway's tokio task
   pushes incoming protocol-stack messages into an inbound bridge
   channel; the inbound gateway item resolves the channel by topic
   match (with wildcard demultiplexing) and re-publishes the envelope
   into the inbound iceoryx2 service.

   .. mermaid::

      sequenceDiagram
        autonumber
        participant B as Broker
        participant MQ as rumqttc::EventLoop
        participant BR as Tokio bridge
        participant GI as InboundGatewayItem
        participant P as Publisher (gateway, in svc)
        participant SHM as iceoryx2 SHM
        participant S as Subscriber (app)
        participant R as ChannelReader
        participant U as user code

        B->>MQ: PUBLISH (topic, payload)
        MQ-->>BR: tokio task pushes (topic, payload) into bridge_in
        BR->>GI: taktora-executor Channel wakes item
        GI->>GI: resolve channel by topic match (wildcard demux)
        GI->>P: publisher.loan(|slot| copy payload, set header)
        P->>SHM: zero-copy publish + notify
        SHM-->>S: WaitSet wakes
        S->>R: reader.try_recv() → Received{ value, header }
        R->>U: user code consumes value

.. architecture:: Health and reconnect lifecycle
   :id: ARCH_0012
   :status: open
   :refines: REQ_0230, REQ_0234, BB_0021

   Every connector implements the same state machine. Concrete
   connectors may add private sub-states, but the externally-visible
   variants are exactly four.

   .. mermaid::

      stateDiagram-v2
        [*] --> Connecting: gateway started
        Connecting --> Up: protocol stack reports connected
        Up --> Degraded: transient error (e.g. PUBACK timeout)
        Degraded --> Up: recovery
        Degraded --> Connecting: recovery episode begins
        Connecting --> Degraded: recover attempt failed
        Up --> Down: stack-level disconnect
        Degraded --> Down: error threshold exceeded
        Down --> Connecting: ReconnectPolicy backoff elapses
        Connecting --> Down: connect attempt fails
        Up --> [*]: shutdown
        Down --> [*]: shutdown

   Every transition emits a ``HealthEvent`` on the connector's health
   channel; the host can forward these into ``taktora_executor::Observer``
   via the optional ``tracing``-feature adapter.

.. architecture:: Shutdown coordination
   :id: ARCH_0013
   :status: open
   :refines: REQ_0243, BB_0005, BB_0021

   Shutdown is downstream of taktora-executor: when ``Executor::run()``
   returns (signal or programmatic stop), the host triggers the tokio
   runtime's ``shutdown_timeout(5s)`` (configurable). Out-of-process
   gateway binaries follow the same pattern; the app side detects loss
   via ``HealthEvent::Down``.

   .. mermaid::

      sequenceDiagram
        autonumber
        participant SIG as SIGINT/SIGTERM
        participant EX as taktora-executor WaitSet
        participant HO as ConnectorHost / Gateway
        participant GI as Gateway items
        participant TT as Tokio runtime
        participant B as Broker

        SIG->>EX: signal delivered
        EX->>EX: WaitSet returns Interrupt
        EX->>HO: Executor::run() returns
        HO->>GI: drop items
        HO->>TT: shutdown_handle.send(())
        TT->>B: client.disconnect() (best-effort)
        B-->>TT: DISCONNECT ack (or timeout)
        TT->>TT: tokio runtime drained
        HO-->>SIG: process exits

.. architecture:: EtherCAT bus bring-up sequence
   :id: ARCH_0040
   :status: open
   :refines: REQ_0313, REQ_0314, REQ_0315, BB_0032, BB_0033

   Bring-up walks the four EtherCAT bus states. PDO mapping is applied
   during the PRE-OP → SAFE-OP transition — the only window where SDO
   writes to the sync-manager assignment indices land on a stable
   mailbox but the cyclic process image is not yet live. Plugin
   traffic is accepted only after the bus reaches OP.

   .. mermaid::

      sequenceDiagram
        autonumber
        participant HO as ConnectorHost
        participant GW as EthercatGateway
        participant EC as ethercrab MainDevice
        participant SD as SubDevices
        participant PL as Plugin (EthercatConnector)

        HO->>GW: start gateway
        GW->>EC: PduStorage.try_split + MainDevice::new
        GW->>EC: spawn tx_rx_task on tokio sidecar
        GW->>EC: init_single_group (INIT to PRE-OP)
        EC->>SD: discover and address SubDevices
        GW->>SD: SDO writes 0x1C12 / 0x1C13 (PDO mapping)
        GW->>EC: group.into_safe_op (PRE-OP to SAFE-OP)
        GW->>EC: group.into_op (SAFE-OP to OP)
        GW-->>HO: ConnectorHealth::Up
        PL->>GW: writer.send / reader.try_recv accepted

.. architecture:: Cyclic process-data exchange and working-counter health
   :id: ARCH_0041
   :status: open
   :refines: REQ_0316, REQ_0317, REQ_0319, REQ_0320, BB_0032, BB_0034

   The gateway runs one ``group.tx_rx`` cycle per tick on
   ``tokio::time::interval`` with ``MissedTickBehavior::Skip``.
   Working-counter inspection on each completed cycle drives the
   externally observable ``ConnectorHealth`` transitions; the resulting
   state machine matches :need:`ARCH_0012` (uniform health surface)
   with EtherCAT-specific entry conditions.

   .. mermaid::

      stateDiagram-v2
        [*] --> Connecting: gateway started
        Connecting --> Up: bus reaches OP and WKC matches
        Up --> Degraded: WKC below expected on N consecutive cycles
        Degraded --> Up: WKC restored
        Up --> Down: bus drops below OP
        Degraded --> Down: WKC remains below expected past timeout
        Down --> Connecting: ReconnectPolicy backoff elapses
        Connecting --> Down: bring-up attempt fails
        Up --> [*]: shutdown
        Down --> [*]: shutdown

.. architecture:: Optional Distributed Clocks bring-up
   :id: ARCH_0042
   :status: open
   :refines: REQ_0318, BB_0032

   When ``EthercatConnectorOptions::distributed_clocks`` is enabled,
   the gateway inserts a register-level bring-up step between the
   PRE-OP and SAFE-OP transitions of :need:`ARCH_0040`. Each step
   uses standard EtherCAT broadcast or configured-write commands; the
   sequence runs once per fresh bring-up and once per ReconnectPolicy-
   driven re-bringup. When DC is disabled, the entire sequence is
   skipped.

   .. mermaid::

      sequenceDiagram
        autonumber
        participant GW as EthercatGateway
        participant EC as ethercrab MainDevice
        participant SD as SubDevices

        GW->>EC: read SupportFlags (0x0008) per SubDevice
        EC->>SD: BRD 0x0008
        SD-->>EC: 64-bit DC support flags
        GW->>EC: latch port-0 receive times
        EC->>SD: BWR 0x0900 (system time)
        loop per DC-capable SubDevice
            EC->>SD: read 0x0918 (receive time processing unit)
            SD-->>EC: t_recv
            EC->>SD: write 0x0920 (system time offset = t_sys - t_recv)
        end
        GW->>EC: propagate reference clock
        EC->>SD: FRMW 0x0910 (first SubDevice clock)

.. architecture:: Zenoh query request/response flow
   :id: ARCH_0043
   :status: open
   :refines: REQ_0420, REQ_0421, REQ_0423, REQ_0424, BB_0042, BB_0043

   The multi-reply request/response flow for Zenoh queries, derived
   from :need:`BB_0043` and :need:`ADR_0043`. ``ZenohQuerier::send``
   mints a ``QueryId``, stamps it into the envelope ``correlation_id``,
   and publishes on ``{name}.query.out``; the gateway issues
   ``zenoh::get`` and holds the ``correlation_id → zenoh::Query`` map.
   On the responder side, ``ZenohQueryable`` drains ``{name}.query.in``,
   replies zero or more data chunks (``payload[0] = 0x01``) on
   ``{name}.reply.out``, then ``terminate`` (``0x02``); a
   gateway-synthetic timeout terminator (``0x03``) closes the stream
   when the query times out. Codecs run plugin-side only; the gateway
   forwards raw bytes.

   .. mermaid::

      sequenceDiagram
        autonumber
        participant QU as user code (querier side)
        participant ZQ as ZenohQuerier
        participant SHM as iceoryx2 SHM
        participant GW as ZenohGateway
        participant ZN as zenoh::Session
        participant RGW as ZenohGateway (responder)
        participant ZA as ZenohQueryable
        participant RU as user code (queryable side)

        QU->>ZQ: send(q) → mints QueryId
        ZQ->>SHM: publish {name}.query.out (correlation_id = QueryId, codec(q))
        SHM-->>GW: WaitSet wakes
        GW->>ZN: session.get(key_expr, payload)
        ZN->>RGW: query delivered to queryable
        RGW->>SHM: publish {name}.query.in (QueryId, q bytes)
        SHM-->>ZA: ZenohQueryable.try_recv() → (QueryId, q)
        ZA->>RU: user handles request, produces R chunks
        loop per reply chunk
            RU->>ZA: reply(id, r)
            ZA->>SHM: publish {name}.reply.out (payload[0]=0x01, codec(r))
            SHM-->>RGW: gateway forwards chunk
            RGW->>ZN: query.reply(sample)
            ZN->>GW: reply sample arrives on querier session
            GW->>SHM: publish {name}.reply.in (payload[0]=0x01, r bytes)
            SHM-->>ZQ: ZenohQuerier.try_recv() → data chunk
            ZQ->>QU: decode(R) chunk delivered
        end
        RU->>ZA: terminate(id)
        ZA->>SHM: publish {name}.reply.out (payload[0]=0x02 end-of-stream)
        SHM-->>RGW: gateway drops zenoh::Query handle
        GW->>SHM: publish {name}.reply.in (payload[0]=0x02 end-of-stream)
        SHM-->>ZQ: try_recv() → end-of-stream
        ZQ->>QU: stream complete

.. architecture:: CAN frame send path (app → bus)
   :id: ARCH_0060
   :status: open
   :refines: REQ_0613, REQ_0621, BB_0072, BB_0073

   The CAN send path is the framework's zero-copy publish path
   (:need:`ARCH_0010`) terminated by a SocketCAN write. The codec
   writes envelope payload bytes directly into shared memory via
   ``Publisher::loan``; the gateway pulls them off the outbound
   bridge and constructs a ``CanFrame`` / ``CanFdFrame`` per
   :need:`REQ_0613`. No re-encoding occurs on the gateway side.

   .. mermaid::

      sequenceDiagram
        autonumber
        participant U as user code
        participant W as ChannelWriter
        participant P as Publisher (taktora-executor)
        participant SHM as iceoryx2 SHM
        participant S as Subscriber (gateway)
        participant GI as OutboundGatewayItem
        participant BR as Tokio bridge (per-iface outbound)
        participant SK as socketcan socket (iface)
        participant BUS as CAN bus

        U->>W: writer.send(&value)
        W->>P: publisher.loan(|slot| codec.encode(value, slot.payload))
        P->>SHM: zero-copy publish + notify
        SHM-->>S: WaitSet wakes
        S->>GI: ExecutableItem::execute()
        GI->>BR: bridge_tx.try_send(payload, routing)
        BR-->>SK: tokio task drains bridge
        SK->>SK: build CanFrame{id, ext, data} or CanFdFrame{id, brs, esi, data}
        SK->>BUS: write_frame()

.. architecture:: CAN receive path with multi-iface demux
   :id: ARCH_0061
   :status: open
   :refines: REQ_0614, REQ_0620, REQ_0622, REQ_0624, BB_0072, BB_0074

   The CAN receive path applies the per-interface kernel filter
   (compiled by :need:`BB_0074` from the union of registered
   readers' ``(can_id, mask, extended)``) before user space sees
   the frame. The gateway then demultiplexes each received frame
   to every channel whose filter matches, re-publishing the data
   bytes onto each matching reader's inbound iceoryx2 service.
   Error frames are siphoned off the same read into the health
   classifier and never reach a plugin channel (:need:`REQ_0631`,
   :need:`REQ_0636`).

   .. mermaid::

      sequenceDiagram
        autonumber
        participant BUS as CAN bus
        participant SK as socketcan socket (iface)
        participant RX as RX task (tokio)
        participant FC as filter / demux (BB_0074)
        participant HC as error classifier (BB_0072)
        participant BR as Tokio bridge (per-iface inbound)
        participant GI as InboundGatewayItem
        participant P as Publisher (gateway, in svc)
        participant SHM as iceoryx2 SHM
        participant S as Subscriber (app)
        participant R as ChannelReader
        participant U as user code

        BUS->>SK: arbitration + ACK, data frame matches kernel filter
        SK->>RX: read_frame() → CanFrame | CanFdFrame | error frame
        alt error frame
            RX->>HC: classify (passive / warning / bus-off)
            HC->>HC: update per-iface sub-state and aggregate via worst-of
        else data frame
            RX->>FC: lookup matching readers by (can_id, mask, ext)
            loop for each matching reader
                FC->>BR: enqueue (descriptor, data)
            end
            BR->>GI: taktora-executor Channel wakes item
            GI->>P: publisher.loan(|slot| copy payload bytes, set header)
            P->>SHM: zero-copy publish + notify
            SHM-->>S: WaitSet wakes
            S->>R: reader.try_recv() → Received{ value, header }
            R->>U: user code consumes value
        end

.. architecture:: CAN bus health and bus-off recovery
   :id: ARCH_0062
   :status: open
   :refines: REQ_0630, REQ_0632, REQ_0633, REQ_0634, REQ_0635, BB_0072

   Per-interface sub-state machine driven by error-frame
   classification; aggregated into the connector's single visible
   ``ConnectorHealth`` via worst-of (:need:`REQ_0630`). Bus-off
   closes the offending socket and arms
   ``ReconnectPolicy::next_delay()``; the reopen sequence
   re-applies the per-interface filter (:need:`BB_0074`) before
   the sub-state can transition back through ``Connecting``.

   .. mermaid::

      stateDiagram-v2
        [*] --> Connecting: iface socket bound, filter applied
        Connecting --> Up: first successful read or successful send
        Up --> Degraded: error-passive / error-warning frame received
        Degraded --> Up: no further error frames for recovery window
        Up --> Down: bus-off error frame received
        Degraded --> Down: bus-off escalation
        Down --> Connecting: ReconnectPolicy backoff elapses, socket reopened, filter re-applied
        Connecting --> Down: socket reopen / filter apply fails
        Up --> [*]: shutdown
        Down --> [*]: shutdown

   Aggregation rule (:need:`REQ_0630`): any iface ``Down`` while
   another remains ``Up`` surfaces as connector-level ``Degraded``;
   all ifaces ``Down`` surfaces as connector-level ``Down``. Every
   transition emits one ``HealthEvent`` (:need:`REQ_0635`) with
   the offending interface name in the payload.
