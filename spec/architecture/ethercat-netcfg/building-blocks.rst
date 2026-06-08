Building block view
===================

arc42 §5 — the building blocks that realise :need:`FEAT_0080`, showing
the codegen data flow from ``network.yaml`` through the parse and codegen
layers to the generated ``&'static`` tables consumed at runtime.

.. contents:: Sections
   :local:
   :depth: 1

----

Structural overview
-------------------

The toolchain is four layers that depend only leftwards. An integrator
authors ``network.yaml``; the build-script glue (:need:`BB_0096` is the
SM-watchdog slice) drives the parse and codegen layers into ``OUT_DIR``;
the consumer crate pulls in the emitted module via ``include!``.

.. mermaid::

   graph LR
       yaml["network.yaml<br/>(integrator-authored)"]
       esi["ESI files<br/>(vendored, pinned)"]
       parser["ethercat-netcfg<br/>(parse layer)<br/>depends on ethercat-esi<br/>+ fieldbus-od-core"]
       ir["NetworkConfig IR<br/>(typed, in-memory)"]
       codegen["ethercat-netcfg-codegen<br/>(codegen layer)<br/>IR → TokenStream<br/>prettyplease formatting"]
       build["ethercat-netcfg-build<br/>(build-script glue)<br/>invoked from build.rs<br/>writes to OUT_DIR"]
       cli["ethercat-netcfg-cli<br/>(netcfg expand / fetch)<br/>vendor-and-pin action"]
       out["OUT_DIR/network.rs<br/>pub static PDO_MAP<br/>EthercatRouting consts<br/>bring-up assertions"]
       consumer["consumer crate<br/>include!(…/network.rs)"]

       yaml --> parser
       esi --> parser
       parser --> ir
       ir --> codegen
       codegen --> build
       build --> out
       out --> consumer
       yaml -.->|"netcfg fetch"| cli
       cli -.->|"vendors + pins"| esi

----

5. Building blocks
------------------

The SM-watchdog resolution and validation building block is the only
arc42 building block named in this v1 slice; the broader parse / codegen
pipeline is described by the capability-cluster features
(:need:`FEAT_0081` – :need:`FEAT_0085`) and their decisions.

.. building-block:: ethercat-netcfg SM-watchdog resolution and validation
   :id: BB_0096
   :status: open
   :implements: FEAT_0085
   :refines: REQ_0844, REQ_0845

   The parse-layer code that, at ``resolve()`` time, resolves and
   validates each output device's sync-manager watchdog (the netcfg slice
   of :need:`AOU_0016`). For every device carrying output (rx) PDOs it
   computes an effective timeout — the per-device ``sm_watchdog_timeout``
   override if present, else FTTI/2 — quantizes it to the ESC registers
   ``0x0400`` / ``0x0420`` (divider 2498 → 100 µs ticks,
   ``intervals = ceil(timeout_us / 100)``, clamped ``1..=u16::MAX``), and
   exposes the resolved ``(divider, intervals)`` on the IR for codegen.
   The quantization arithmetic is a deliberate ~5-line duplicate of the
   connector's ``SmWatchdog`` (a dependency on the connector runtime was
   rejected per :need:`REQ_0824`). Validation rejects an effective window
   that exceeds FTTI/2 (checked against the QUANTIZED value, since
   ``ceil`` can push a boundary request over the bound), an ESI-sourced
   output SM whose watchdog trigger is disabled, and an inline-sourced
   output device that does not attest ``sm_watchdog_enabled: true``.
   Input-only devices are untouched. Implemented in
   ``crates/taktora-ethercat-netcfg/src/lib.rs`` and
   ``crates/taktora-ethercat-netcfg-codegen/src/lib.rs``.
