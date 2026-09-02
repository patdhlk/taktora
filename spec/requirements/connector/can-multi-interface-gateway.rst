Multi-interface gateway and per-channel filtering
=================================================

The gateway-side multiplexer. This cluster ``:satisfies:`` :need:`FEAT_0046`.

.. feat:: Multi-interface gateway and per-channel filtering
   :id: FEAT_0048
   :status: open
   :satisfies: FEAT_0046

   The gateway-side multiplexer: one gateway instance can own
   multiple Linux CAN interfaces (broader than :need:`REQ_0312`'s
   single-MainDevice EtherCAT posture). Per-channel CAN ID and mask
   are compiled into one ``CAN_RAW_FILTER`` ``setsockopt`` per
   interface, recomputed when channels are added or removed.

.. req:: Multiple interfaces per gateway
   :id: REQ_0620
   :status: implemented
   :satisfies: FEAT_0048
   :links: BB_0072, IMPL_0080, TEST_0505

   A single ``CanGateway`` instance shall be capable of owning
   multiple Linux SocketCAN interfaces (e.g. ``can0``, ``can1``,
   ``vcan0``) simultaneously. The set of interfaces shall be
   declared at gateway construction via
   ``CanConnectorOptions::ifaces``. This requirement is broader
   than :need:`REQ_0312`'s single-MainDevice EtherCAT posture
   because SocketCAN bus saturation per interface is far lower
   than EtherCAT process-image throughput and multi-bus
   deployments are common in CAN.

.. req:: Routing identifies the interface
   :id: REQ_0621
   :status: implemented
   :satisfies: FEAT_0048
   :links: BB_0071, IMPL_0080, TEST_0505

   ``CanRouting::iface`` shall identify which gateway-owned
   SocketCAN interface a channel binds to. The gateway shall
   reject ``Connector::create_writer`` / ``create_reader`` calls
   referencing an interface not listed in
   ``CanConnectorOptions::ifaces`` with
   ``ConnectorError::Configuration``.

.. req:: Per-interface filter is the union of channel masks
   :id: REQ_0622
   :status: implemented
   :satisfies: FEAT_0048
   :links: BB_0074, IMPL_0080, TEST_0504

   For each owned interface, the gateway shall compute the union
   of ``(can_id, mask, extended)`` tuples drawn from every
   currently-open inbound channel bound to that interface, and
   apply the result as a single ``setsockopt(SOL_CAN_RAW,
   CAN_RAW_FILTER, …)`` call. Frames not matching any registered
   filter shall be discarded by the kernel before reaching the
   gateway's read loop.

.. req:: Filter recomputed on channel add/remove
   :id: REQ_0623
   :status: implemented
   :satisfies: FEAT_0048
   :links: BB_0074, IMPL_0080, TEST_0504

   The per-interface filter (per :need:`REQ_0622`) shall be
   recomputed and re-applied whenever a ``ChannelReader`` is
   created or dropped. The recompute shall not require the
   interface to be re-opened or the bus to leave its current
   state.

.. req:: Inbound demux to all matching readers
   :id: REQ_0624
   :status: implemented
   :satisfies: FEAT_0048
   :links: BB_0072, BB_0074, IMPL_0080, TEST_0505

   When a CAN frame arrives on an interface, the gateway shall
   publish the frame's data bytes (per :need:`REQ_0614`) onto the
   inbound iceoryx2 service of every registered channel whose
   ``(iface, can_id, mask, extended)`` matches the received
   frame's identifier under kernel ``CAN_RAW_FILTER`` semantics.
   Overlapping channel filters shall each receive their own
   envelope copy.

.. req:: Per-iface routing registry has stable iteration order
   :id: REQ_0625
   :status: implemented
   :satisfies: FEAT_0048
   :links: BB_0072, IMPL_0080, TEST_0514

   The gateway shall maintain a per-interface routing registry
   mapping each open ``ChannelDescriptor`` to its ``CanRouting``
   and direction (outbound writer / inbound reader). The RX/TX
   loops shall iterate this registry on every frame and every
   send drain without per-iteration heap allocation (no ``Vec``
   resize, no ``HashMap`` re-hash) — required by :need:`REQ_0060`
   from the steady-state posture, mirroring :need:`REQ_0328`.
