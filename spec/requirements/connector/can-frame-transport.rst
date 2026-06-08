CAN frame transport (classical + FD)
====================================

The on-wire form of CAN traffic crossing the plugin↔gateway boundary. This
cluster ``:satisfies:`` :need:`FEAT_0046`.

.. feat:: CAN frame transport (classical + FD)
   :id: FEAT_0047
   :status: open
   :satisfies: FEAT_0046

   The on-wire form of CAN traffic crossing the plugin↔gateway
   boundary. ``CanRouting`` declares per-channel CAN ID, mask, and
   frame kind (Classical or FD); the iceoryx2 service payload
   carries the raw CAN data bytes (codec-encoded plugin-side per
   :need:`REQ_0211`), with the gateway acting as a byte-only mover
   (symmetric with :need:`REQ_0327` and :need:`REQ_0408`).

.. req:: Classical CAN frames supported
   :id: REQ_0610
   :status: approved
   :satisfies: FEAT_0047

   For channels declared with ``CanFrameKind::Classical``, the
   connector shall transport up to 8 bytes of payload per frame,
   with 11-bit standard or 29-bit extended identifiers (per
   :need:`REQ_0601`'s ``extended`` flag). The corresponding
   iceoryx2 service payload capacity shall be 8 bytes plus the
   ``ConnectorEnvelope`` header.

.. req:: CAN-FD frames supported
   :id: REQ_0611
   :status: approved
   :satisfies: FEAT_0047

   For channels declared with ``CanFrameKind::Fd``, the connector
   shall transport up to 64 bytes of payload per frame with the
   FD-specific bit-rate-switch (BRS) and error-state-indicator
   (ESI) flags carried in ``CanFdFlags``. The corresponding
   iceoryx2 service payload capacity shall be 64 bytes plus the
   ``ConnectorEnvelope`` header.

.. req:: Channel payload sizing keyed on frame kind
   :id: REQ_0612
   :status: open
   :satisfies: FEAT_0047

   ``ChannelDescriptor<CanRouting>::max_payload_size`` shall be
   derived deterministically from ``CanRouting::kind``: 8 bytes
   for ``Classical``, 64 bytes for ``Fd``. The framework shall
   reject any plugin-provided override that violates this mapping
   with ``ConnectorError::Configuration``.

.. req:: Outbound payload serialised to socketcan frame
   :id: REQ_0613
   :status: approved
   :satisfies: FEAT_0047

   When a plugin publishes a value through ``ChannelWriter::send``,
   the gateway shall, before the next RX/TX iteration on the
   target interface, construct a ``socketcan::CanFrame`` (for
   ``Classical``) or ``socketcan::CanFdFrame`` (for ``Fd``) whose
   identifier is the channel's ``CanRouting::can_id`` (with the
   ``extended`` flag honoured), whose data bytes are the
   codec-encoded envelope payload, whose DLC is the payload
   length, and — for FD — whose BRS / ESI flags are copied from
   ``CanRouting::fd_flags``. The gateway shall not re-encode the
   payload.

.. req:: Inbound gateway is byte-only on the publish path
   :id: REQ_0614
   :status: approved
   :satisfies: FEAT_0047

   On the inbound leg (CAN bus → plugin), the gateway shall
   publish the raw frame data bytes received from the SocketCAN
   read onto the matching channel's inbound iceoryx2 service as a
   ``ConnectorEnvelope`` without invoking the channel's codec —
   codec decoding is the responsibility of the plugin-side
   ``ChannelReader::try_recv`` (symmetric with :need:`REQ_0327`
   and :need:`REQ_0408`).

.. req:: CAN ID extended flag preserved end-to-end
   :id: REQ_0615
   :status: approved
   :satisfies: FEAT_0047

   The ``CanRouting::can_id.extended`` boolean shall be preserved
   end-to-end between plugin and gateway: outbound, the gateway
   shall set the ``CAN_EFF_FLAG`` bit on the kernel-side frame iff
   ``extended`` is true; inbound, the gateway shall match against
   ``can_id`` and ``mask`` honouring the same flag distinction so
   that 11-bit and 29-bit IDs occupying the same numeric value are
   delivered to separate readers.
