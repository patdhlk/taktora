Risks and glossary
==================

arc42 §11–§12.

11. Risks and technical debt
----------------------------

.. risk:: rumqttc API stability before 1.0
   :id: RISK_0001
   :status: open
   :links: BB_0021, ADR_0001

   ``rumqttc`` is the chosen MQTT crate but is pre-1.0; minor releases
   may break API. **Mitigation:** pin to a specific 0.x.y in
   ``Cargo.toml``; document the version in ``MqttConnectorOptions``
   docs; gate upgrades behind running the MQTT integration suite.

.. risk:: iceoryx2 0.8 pre-1.0 churn
   :id: RISK_0002
   :status: open
   :links: BB_0002, CON_0002

   iceoryx2 0.8.x is itself pre-1.0 and changes shape between minor
   versions. **Mitigation:** workspace pins ``iceoryx2 = "0.8"``;
   upgrades are an explicit follow-on effort across the entire
   workspace.

.. risk:: Const-generic monomorphisation cost
   :id: RISK_0003
   :status: open
   :links: BB_0010, ADR_0004

   ``ConnectorEnvelope<const N: usize>`` produces a distinct type per
   ``N``; an application with many channel sizes can grow code size.
   **Mitigation:** if profiling shows monomorphisation overhead, the
   plan-stage may substitute a small set of size-tier types (4 KB /
   64 KB / 1 MB) without breaking the external contract.

.. risk:: Tokio bridge latency
   :id: RISK_0004
   :status: open
   :links: BB_0022, ADR_0007

   The taktora-executor↔tokio bridge adds a channel hop on every
   message in both directions. **Mitigation:** the bridge stays in the
   gateway process (not crossing SHM); benchmarks at plan stage
   characterise added latency; if intolerable, a follow-on can move
   the rumqttc EventLoop directly onto a taktora-executor item triggered
   from a raw socket.

.. risk:: Wildcard demux pathological topic patterns
   :id: RISK_0005
   :status: open
   :links: REQ_0254, BB_0021

   MQTT wildcard subscriptions (``+``, ``#``) can produce many channel
   matches per inbound message. **Mitigation:** the gateway's demux
   structure (trie, flat-vec, hash-of-prefixes — chosen at plan stage)
   is proptest'd for equivalence; integration tests cover overlapping
   wildcard scenarios.

----

12. Glossary
------------

.. term:: Connector
   :id: GLOSS_0001
   :status: open

   A pair of (plugin, gateway) that bridges a taktora-executor
   application to one external protocol family (MQTT, OPC UA, gRPC,
   ADS, ...). One concrete crate per protocol; all connectors share
   the framework's five contracts.

.. term:: Plugin
   :id: GLOSS_0002
   :status: open

   The in-app side of a connector. A type implementing
   ``Connector`` that user code obtains channel handles from. Lives
   in the application's own process; speaks no network.

.. term:: Gateway
   :id: GLOSS_0003
   :status: open

   The out-of-app side of a connector. Hosts the actual protocol
   stack (e.g. ``rumqttc::EventLoop``) on a tokio runtime sidecar and
   exposes itself to taktora-executor as a set of ``ExecutableItem``
   instances. Deployed in-process or as a separate binary.

.. term:: ConnectorEnvelope
   :id: GLOSS_0004
   :status: open

   The on-wire form of every message crossing the plugin↔gateway
   boundary. ``#[repr(C)]`` POD with header + const-generic-sized
   payload. See :need:`BB_0010`.

.. term:: Codec
   :id: GLOSS_0005
   :status: open

   A type implementing ``PayloadCodec`` that converts user values to
   payload bytes and back. Selected at compile time as a generic
   parameter on the connector type. See :need:`BB_0003`,
   :need:`ARCH_0030`.

.. term:: Routing
   :id: GLOSS_0006
   :status: open

   A protocol-typed struct (``MqttRouting``, ``OpcUaRouting``, ...)
   embedded in ``ChannelDescriptor`` that tells the gateway how to
   address external endpoints (MQTT topic, OPC UA NodeId, gRPC
   method, ...). See :need:`ADR_0008`.

.. term:: Bridge
   :id: GLOSS_0007
   :status: open

   The bounded-channel pair connecting taktora-executor's WaitSet
   thread to the tokio runtime inside a connector crate. Outbound
   bridge is ``tokio::sync::mpsc``; inbound bridge is
   ``crossbeam_channel`` wired as a taktora-executor signal source.
   See :need:`BB_0022`.

.. term:: Health
   :id: GLOSS_0008
   :status: open

   The four-state observable lifecycle of a connector
   (``Up`` / ``Connecting`` / ``Degraded`` / ``Down``) emitted as
   ``HealthEvent`` on the connector's health channel. Uniform across
   protocols; see :need:`ARCH_0012`.

.. term:: Reconnect policy
   :id: GLOSS_0009
   :status: open

   A ``ReconnectPolicy`` implementation (default
   ``ExponentialBackoff``) used by connectors whose protocol stack
   exposes raw connect events. Stacks that manage reconnect
   internally do not use ``ReconnectPolicy`` but still emit
   ``HealthEvent`` (:need:`REQ_0235`).

.. term:: Channel
   :id: GLOSS_0010
   :status: open

   A logical bidirectional or unidirectional flow named by
   ``ChannelDescriptor::name``. Each channel direction maps to one
   iceoryx2 publish-subscribe service plus an event service for
   wakeups. Per-channel max payload size is fixed at
   service-creation time (:need:`ADR_0004`).

.. term:: ASIL
   :id: GLOSS_0011
   :status: open

   Automotive Safety Integrity Level (ISO 26262). Cited only for
   context in :need:`QG_0001` — the connector framework is a useful
   shape for keeping non-deterministic protocol code OUT of an
   ASIL-rated control loop, but the framework itself makes no safety
   integrity claims.
