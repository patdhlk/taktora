Connector framework
===================

This chapter captures the requirements for ``taktora-connector``: a framework
that connects taktora-executor applications to external protocols (MQTT, OPC UA,
gRPC, fieldbus) through a controlled boundary, so messy network code lives
outside the application's deterministic core.

The decomposition is two-tier:

* **Top-level umbrella feature** — :need:`FEAT_0030` — peer to
  :need:`FEAT_0010`. Taktora-connector is a general-purpose framework usable
  by any taktora-executor consumer; it is not bound to the PLC use case.
* **Capability-cluster sub-features** — one per architectural concern, each
  ``:satisfies:`` :need:`FEAT_0030`.
* **Requirements** — concrete shall-clauses that ``:satisfies:`` a
  capability-cluster feature.

This round covers the framework core plus an MQTT reference connector
(``rumqttc``-backed). OPC UA, gRPC, and Beckhoff ADS connectors are
deferred to follow-on specs that will reuse the same five contracts.

Each capability cluster lives on its own page (see the toctree): envelope
transport (:need:`FEAT_0031`), codec abstraction (:need:`FEAT_0032`), the
connector trait and routing (:need:`FEAT_0033`), connection lifecycle
(:need:`FEAT_0034`), process-boundary deployments (:need:`FEAT_0035`),
connector cycle telemetry (:need:`FEAT_0038`), host wiring and builder
(:need:`FEAT_0037`), and the reference connectors — MQTT (:need:`FEAT_0036`),
EtherCAT (:need:`FEAT_0041`), Zenoh (:need:`FEAT_0042`), and CAN
(:need:`FEAT_0046`). The deliberately rejected anti-goals, the umbrella-level
traceability tables, and the safety refinements live on :doc:`cross-cutting`.

Top-level umbrella
------------------

.. feat:: Connector framework
   :id: FEAT_0030
   :status: open

   A Rust framework that bridges taktora-executor applications to external
   protocols through a typed envelope carried over iceoryx2 shared memory.
   The framework provides five contracts — envelope, codec, routing, health,
   lifecycle — that every protocol connector instantiates as a plugin
   (in-app side) and a gateway (out-of-app side). Both halves are
   taktora-executor ``ExecutableItem`` consumers; protocol-specific async
   work runs on a tokio sidecar contained inside each connector crate.

   Deployment chooses whether the gateway runs as a tokio task in-process
   alongside the plugin host, or as a separate gateway binary. The envelope
   contract is identical either way; only process-startup wiring differs.

   This umbrella is a peer of :need:`FEAT_0010` "PLC runtime heart"; the
   connector framework is a general-purpose mechanism, not PLC-specific.
   :need:`FEAT_0023` "Fieldbus integration interface" is later expected to
   ``:refines:`` this umbrella once a fieldbus connector spec lands.

Requirements at a glance
------------------------

.. needtable::
   :columns: id, title, status, satisfies
   :show_filters:
   :filter: "FEAT_0030" in satisfies or "FEAT_0031" in satisfies or "FEAT_0032" in satisfies or "FEAT_0033" in satisfies or "FEAT_0034" in satisfies or "FEAT_0035" in satisfies or "FEAT_0036" in satisfies or "FEAT_0037" in satisfies or "FEAT_0038" in satisfies or "FEAT_0041" in satisfies or "FEAT_0042" in satisfies or "FEAT_0043" in satisfies or "FEAT_0044" in satisfies or "FEAT_0045" in satisfies or "FEAT_0046" in satisfies or "FEAT_0047" in satisfies or "FEAT_0048" in satisfies or "FEAT_0049" in satisfies

.. toctree::
   :maxdepth: 2

   envelope-transport
   codec
   trait-routing
   connection-lifecycle
   process-boundary
   mqtt
   ethercat
   host-wiring
   zenoh
   can
   telemetry
   cross-cutting
