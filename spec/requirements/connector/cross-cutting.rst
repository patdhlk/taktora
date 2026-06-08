Cross-cutting and anti-goals
============================

This page collects the framework-wide concerns that span every capability
cluster: the deliberately rejected anti-goals, the umbrella-level
traceability tables, and the safety refinements.

Anti-goals
----------

The following requirements are explicitly **rejected** — captured for the
record so that future readers see what the framework deliberately does
not do, and why. Each rejected requirement ``:satisfies:`` :need:`FEAT_0030`
to keep the umbrella's traceability complete.

.. req:: NO request/response matching by the framework
   :id: REQ_0290
   :status: rejected
   :satisfies: FEAT_0030

   The framework shall **not** match requests to responses using
   ``ConnectorEnvelope::correlation_id``. The field is a passive carrier;
   higher-layer code may use it for correlation, but the framework
   performs no inspection or matching.

.. req:: NO app↔gateway control plane
   :id: REQ_0291
   :status: rejected
   :satisfies: FEAT_0030

   The framework shall **not** introduce envelopes carrying ``ping``,
   ``version-negotiation``, or ``shutdown-handshake`` semantics across
   the plugin↔gateway boundary. Health and lifecycle are observed via
   ``ConnectorHealth``, not negotiated through SHM control-plane
   envelopes.

.. req:: NO persistent outbox or durable buffering
   :id: REQ_0292
   :status: rejected
   :satisfies: FEAT_0030

   The framework shall **not** persist outbound envelopes on disk or in
   any durable store when the gateway is ``Down``. ``ChannelWriter::send``
   shall return ``Err(Down)`` instead. Durability is the responsibility
   of the broker (MQTT QoS 1/2) or an application-level outbox layered
   above the connector.

.. req:: NO schema/contract enforcement across the boundary
   :id: REQ_0293
   :status: rejected
   :satisfies: FEAT_0030

   The framework shall **not** verify that plugin and gateway agree on
   the channel's payload type ``T`` or codec ``C``. Mismatch surfaces
   only as a decode failure; the framework offers no central schema
   registry.

.. req:: NO protocol-portable Channel<T>
   :id: REQ_0294
   :status: rejected
   :satisfies: FEAT_0030

   The framework shall **not** offer a channel type that is portable
   between protocols ("write the same plugin code, swap MQTT for OPC UA
   without code changes"). Plugin code imports its connector's
   ``Routing`` and is concrete about which protocol it targets.

.. req:: NO multi-broker / multi-tenant gateway
   :id: REQ_0295
   :status: rejected
   :satisfies: FEAT_0030

   A single ``MqttGateway`` instance shall connect to at most one MQTT
   broker. Multi-broker deployments shall instantiate multiple gateways.

.. req:: NO supervision / panic recovery
   :id: REQ_0296
   :status: rejected
   :satisfies: FEAT_0030

   The framework shall **not** catch panics from the tokio task or any
   protocol-stack worker. A panic shall propagate and abort the gateway
   process; restart policy is the host's responsibility, matching
   taktora-executor's existing posture.

.. req:: NO DBC parsing or typed signal extraction in taktora-connector-can
   :id: REQ_0640
   :status: rejected
   :satisfies: FEAT_0046

   The CAN connector shall **not** parse Vector DBC files or perform
   bit-/signal-level extraction from CAN payloads. The connector is
   a raw-frame transport; typed signal codecs are a separate concern
   for a future feature layered on top.

.. req:: NO ISO-TP or J1939 support in taktora-connector-can
   :id: REQ_0641
   :status: rejected
   :satisfies: FEAT_0046

   The CAN connector shall **not** implement ISO-TP (ISO 15765-2)
   segmentation or J1939 (PGN, transport protocol, address claim).
   Applications needing higher-layer CAN protocols shall either
   layer them above ``CanConnector`` or open a separate
   ``CAN_ISOTP`` / ``CAN_J1939`` socket family connector in a
   follow-on spec.

.. req:: NO CAN-XL support
   :id: REQ_0642
   :status: rejected
   :satisfies: FEAT_0046

   The CAN connector shall **not** transport CAN-XL (CiA 610-1)
   frames. The first cut targets classical CAN and CAN-FD only;
   CAN-XL is deferred to a follow-on spec once the underlying
   ``socketcan`` crate and the Linux kernel surface stabilise.

.. req:: NO plugin-visible error-frame channel
   :id: REQ_0643
   :status: rejected
   :satisfies: FEAT_0049

   The CAN connector shall **not** expose CAN error frames as a
   plugin-readable ``ChannelReader``. Error-frame consumption
   stays inside the gateway and surfaces only through
   ``ConnectorHealth`` / ``HealthEvent`` (re-affirms
   :need:`REQ_0636`).

.. req:: NO can-restart-ms management from the gateway
   :id: REQ_0644
   :status: rejected
   :satisfies: FEAT_0049

   The CAN connector shall **not** set the kernel's
   ``can-restart-ms`` netlink attribute on owned interfaces.
   Interface bring-up (``ip link set canX up type can …``) and
   auto-restart configuration remain a host / sysadmin concern;
   ``taktora-connector-can`` only opens the already-up interface.

Cross-cutting traceability
--------------------------

Every requirement in this chapter (excluding rejected anti-goals) carries a
``:satisfies:`` link to its capability-cluster feat; every cluster feat
``:satisfies:`` :need:`FEAT_0030`. Architectural specifications
(``spec`` directives) refining these requirements are emitted in
:doc:`../../architecture/connector`. Verification artefacts (``test``
directives) are emitted in :doc:`../../verification/connector`.

.. needtable::
   :types: feat
   :filter: "FEAT_003" in id or id in ("FEAT_0041", "FEAT_0042", "FEAT_0043", "FEAT_0044", "FEAT_0045", "FEAT_0046", "FEAT_0047", "FEAT_0048", "FEAT_0049")
   :columns: id, title, status, satisfies
   :show_filters:

.. needtable::
   :types: req
   :filter: "REQ_02" in id or ("REQ_03" in id and id >= "REQ_0310") or "REQ_04" in id or "REQ_05" in id
   :columns: id, title, status, satisfies
   :show_filters:

Safety refinements
------------------

The connector framework carries five TSRs from the SEooC safety
concept (see :doc:`../../safety/tsc`):

* :need:`TSR_0005` (compile-time channel directionality) —
  **implemented** by :need:`BB_0001`, :need:`BB_0005`.
* :need:`TSR_0006` (bounded health-event latency) — **implemented**
  by :need:`REQ_0440`, :need:`REQ_0441`, :need:`REQ_0442`,
  :need:`REQ_0443`, :need:`REQ_0444`.
* :need:`TSR_0007` (single-publisher iceoryx2 topology for SC
  channels) — **implemented** (iceoryx2 default).
* :need:`TSR_0008` (envelope sequence + CRC integrity) — **draft**;
  current ``ConnectorEnvelope<N>`` carries a ``CorrelationId`` but no
  sequence or CRC.
* :need:`TSR_0009` (cross-process hosting mode) — **draft**; requires
  per-process iceoryx2 segment capability wiring at the
  ``ConnectorGateway`` layer. See :need:`ADR_0050`.
