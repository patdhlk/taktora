Overview
========

The ``taktora`` workspace builds a soft-real-time runtime stack on
`iceoryx2`_, in two layered pieces:

* ``taktora-executor`` turns IPC events, intervals, and request/response
  activity into deterministic, observable schedules of executable items.
* ``taktora-connector-*`` exposes typed channels behind a uniform plugin
  surface, so application logic talks to EtherCAT, Zenoh, and any future
  fieldbus through the same ``ChannelReader`` / ``ChannelWriter`` types.

System context
--------------

The diagram below maps the full taktora workspace at a glance; see
:doc:`requirements/plc-runtime/index` (:need:`FEAT_0010`),
:doc:`requirements/connector/index` (:need:`FEAT_0030`),
:doc:`architecture/connector/index`, and
:doc:`architecture/motion/index` for detailed breakdowns of each layer.

.. mermaid::

   %%{init: {"flowchart": {"rankSpacing": 60, "nodeSpacing": 40}}}%%
   flowchart TB

     subgraph BUILD["Build-time codegen (not present at runtime)"]
       direction LR
       ESI["ESI device-codegen\n(FEAT_0050)\nethercat-esi → typed driver structs"]
       CAN_CG["CANopen codegen\n(FEAT_0060)\nEDS → typed driver structs"]
       NETCFG["ethercat-netcfg\n(FEAT_0080)\nnetwork.yaml → &'static bus tables"]
     end

     subgraph RUNTIME["taktora runtime"]
       direction TB
       IOX[("iceoryx2\nzero-copy IPC")]
       EXEC["taktora-executor\ncyclic · event · req-resp dispatch\nof ExecutableItems"]
       IOX <--> EXEC

       subgraph CONN["taktora-connector-* plugin surface\n(ChannelReader / ChannelWriter)"]
         direction LR
         EC["connector-ethercat"]
         ZN["connector-zenoh"]
         CN["connector-can\n(SocketCAN)"]
         MQ["connector-mqtt"]
       end

       EXEC <--> CONN

       MOTION["taktora-motion-core\n(no_std, alloc-free)\nCSP setpoint generation"]
       MOTION -->|"commanded position"| EC
     end

     subgraph INFRA["Cross-cutting workspace infrastructure"]
       direction LR
       ALLOC["taktora-bounded-alloc\n#[global_allocator]\n(FEAT_0040)"]
       LOG["taktora-log + taktora-log-dlt\nlogging facade · DLT backend\n(FEAT_0070)"]
     end

     APP["Application logic\nExecutableItems"]
     APP <--> EXEC

     subgraph EXT["External systems"]
       direction TB
       EBUS["EtherCAT bus\n+ CiA 402 drives (CSP)"]
       ZNET["Zenoh network"]
       CBUS["CAN bus"]
       MQTT["MQTT broker"]
     end

     EC -->|"PDO process data"| EBUS
     ZN <--> ZNET
     CN <--> CBUS
     MQ <--> MQTT

     ESI -.->|"&'static driver tables"| EC
     CAN_CG -.->|"&'static driver tables"| CN
     NETCFG -.->|"&'static bus tables"| EC

     INFRA -.->|"workspace-wide"| RUNTIME

This specification frames the workspace as the **runtime heart of a
soft-real-time PLC**: a foundation for non-safety industrial automation,
robotics control loops, machine-monitoring runtimes, and R&D testbeds where
occasional jitter is acceptable. The framing follows directly from a gap
analysis (recorded in :doc:`requirements/plc-runtime/index`) that distinguishes
the capabilities ``taktora`` already provides from those that must be
added before it can credibly call itself a soft-RT PLC.

What is **out of scope** for the runtime heart:

* Hard-real-time guarantees (cycle deadlines bounded to microsecond jitter
  under any load).
* IEC 61508 / 26262 functional safety certification or SIL / ASIL claims.
* IEC 61131-3 frontends (ladder logic, function block diagram, structured text).
* Hot-standby, redundancy, online change.
* Specific fieldbus protocols (EtherCAT, Modbus, Profinet, CIP); only the
  *integration interface* is in scope.

This document is structured as engineering artefacts. Top-level capabilities
are ``feat`` directives; individual obligations are ``req`` directives that
``:satisfies:`` their parent feature. Every artefact carries a stable ID
(``FEAT_NNNN``, ``REQ_NNNN``) and a lifecycle ``:status:``.

.. _iceoryx2: https://github.com/eclipse-iceoryx/iceoryx2

Reading order
-------------

Start with :doc:`requirements/plc-runtime/index` for the full feature/requirement
decomposition. Work-in-progress sections — :doc:`architecture/index` and
:doc:`verification/index` — will land alongside the implementation work that
closes the gap requirements.

Status legend
-------------

* ``open`` — drafted; not yet reviewed.
* ``draft`` — under active authoring (used during edits).
* ``reviewed`` — passed at least one structured review.
* ``approved`` — accepted into the baseline.

All artefacts in this initial drop are authored at ``open`` until they are
reviewed; the ``status`` lifecycle is honest about provenance rather than
optimistic.
