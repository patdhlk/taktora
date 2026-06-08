Connector framework — architecture (arc42)
==========================================

Architecture documentation for the connector framework, structured per
the arc42 template (12 sections) and encoded with sphinx-needs using
the useblocks "x-as-code" conventions
(https://x-as-code.useblocks.com/how-to-guides/arc42/index.html).

Each architectural element ``:refines:`` or ``:implements:`` a parent
requirement from :doc:`../../requirements/connector/index` so the trace is
preserved end-to-end.

This chapter is split across pages (see the toctree): the framing
sections §1–§3 (goals, constraints, context and scope — including the
system-context mermaid) live here on the index; the solution strategy
and its ADRs (§4) live in :doc:`solution-strategy`; the building-block
decomposition (§5) lives in :doc:`building-blocks`; the runtime
scenarios (§6) live in :doc:`runtime-view`; the two deployment shapes
(§7) live in :doc:`deployment-view`; the crosscutting concepts (§8) and
the cross-cutting traceability tables live in :doc:`crosscutting`; the
architecture-decision and quality-requirement pointers (§9–§10) live in
:doc:`decisions`; the risks and glossary (§11–§12) live in
:doc:`risks`; the crate implementations (§13) live in
:doc:`implementations`; and the connector cycle telemetry design lives
in :doc:`telemetry`.

.. contents:: Sections
   :local:
   :depth: 1

----

1. Introduction and goals
-------------------------

The connector framework's reason-to-exist is fault isolation: keep messy
network protocol code (MQTT, OPC UA, gRPC, fieldbus) outside the
taktora-executor application's deterministic core, while preserving
zero-copy data flow. Quality goals capture the qualities that the
architecture is optimised for.

.. quality-goal:: Fault isolation between protocol stack and app
   :id: QG_0001
   :status: open
   :refines: FEAT_0030

   A panic, hang, or crash in a protocol stack (rumqttc, opcua, tonic,
   ADS) shall not be able to crash, deadlock, or stall the
   taktora-executor application that uses the framework. This goal is
   what motivates the gateway-as-separate-process deployment shape and
   the single-direction control plane.

.. quality-goal:: Compile-time type safety end-to-end
   :id: QG_0002
   :status: open
   :refines: FEAT_0030

   Plugin code that targets a specific protocol shall be checked at
   compile time for routing correctness, codec compatibility, and
   payload-size compliance. Runtime "config-as-strings" indirection
   shall be avoided; type errors are caught by ``cargo check``.

.. quality-goal:: Zero-copy data flow on the publish path
   :id: QG_0003
   :status: open
   :refines: FEAT_0031

   Outbound messages from the application to the broker shall not be
   copied into any intermediate buffer between the codec's encode call
   and the iceoryx2 publish. The iceoryx2 ``Publisher::loan`` mechanism
   carries the codec's output directly to shared memory.

.. quality-goal:: Uniform observable health across connectors
   :id: QG_0004
   :status: open
   :refines: FEAT_0034

   Every connector — regardless of which protocol stack owns its
   reconnect mechanism — shall report the same four health states
   (Up / Connecting / Degraded / Down) on a single observable channel,
   so monitoring and alerting code is connector-agnostic.

----

2. Constraints
--------------

Constraints come from the surrounding workspace and the iceoryx2
ecosystem; they are non-negotiable inputs to the architecture.

.. constraint:: Built on taktora-executor's WaitSet
   :id: CON_0001
   :status: open
   :refines: FEAT_0030

   The plugin and gateway shall be taktora-executor consumers
   (``ExecutableItem``-based, WaitSet-driven). The framework shall not
   introduce a second reactor model running alongside taktora-executor.

.. constraint:: iceoryx2 0.8.x as the IPC layer
   :id: CON_0002
   :status: open
   :refines: FEAT_0030

   The framework shall use the workspace's pinned iceoryx2 version
   (``0.8`` per ``Cargo.toml`` workspace dependencies). Migration to
   a later iceoryx2 series is a follow-on effort outside this spec.

.. constraint:: Rust 2024 edition / MSRV 1.85
   :id: CON_0003
   :status: open
   :refines: FEAT_0030

   All new crates shall target edition 2024 with MSRV 1.85, matching
   the workspace's ``rust-toolchain.toml`` and ``[workspace.package]``.

.. constraint:: Single-threaded test discipline
   :id: CON_0004
   :status: open
   :refines: FEAT_0030

   Workspace tests run with ``--test-threads=1`` because each iceoryx2
   service must own a unique name in shared memory. New crates'
   integration tests shall be safe under this discipline (per-test
   ``Node`` names + per-test tokio runtimes).

.. constraint:: Tokio sidecar contained per connector crate
   :id: CON_0005
   :status: open
   :refines: FEAT_0030

   Where async protocol stacks (``rumqttc``, ``tonic``) require tokio,
   each connector crate shall host its own tokio runtime sidecar; tokio
   shall not appear as a dependency of ``taktora-connector-core``,
   ``taktora-connector-transport-iox``, or ``taktora-connector-codec``.

----

3. Context and scope
--------------------

.. architecture:: System context
   :id: ARCH_0001
   :status: open
   :refines: FEAT_0030

   The connector framework sits between a taktora-executor application
   and one or more external systems (brokers, servers, PLCs).
   Internally, the boundary is split between a **plugin** (in-app side)
   and a **gateway** (out-of-app side); externally, the gateway is the
   only component that touches network I/O.

   .. mermaid::

      flowchart LR
        APP["taktora-executor application<br/>(plugin uses Connector trait)"]
        SHM[("iceoryx2 shared memory<br/>+ event service")]
        GW["taktora-connector gateway<br/>(tokio + protocol stack)"]
        EXT[("external system<br/>e.g. MQTT broker")]
        APP -- ConnectorEnvelope --> SHM
        SHM -- ConnectorEnvelope --> APP
        SHM -- ConnectorEnvelope --> GW
        GW -- ConnectorEnvelope --> SHM
        GW -- protocol native --> EXT
        EXT -- protocol native --> GW

   In-process deployment collapses the SHM hop to a single-process
   shared-memory transport but preserves the same envelope contract;
   see :need:`ARCH_0020` and :need:`ARCH_0021`.

.. toctree::
   :maxdepth: 2

   solution-strategy
   building-blocks
   runtime-view
   deployment-view
   crosscutting
   decisions
   risks
   implementations
   telemetry
