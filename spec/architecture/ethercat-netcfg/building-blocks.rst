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

One building block per crate of the pipeline — parse layer, codegen
layer, build-script glue, CLI — mirroring the per-crate decomposition of
the device-driver toolchain (:need:`BB_0060` – :need:`BB_0066`), plus the
SM-watchdog resolution slice (:need:`BB_0096`), which cuts across the
parse and codegen layers and is kept as its own block because it carries
the safety argument of :need:`AOU_0016`. Each block records the crate's
job, public surface, and the dependencies it is and is not allowed to
carry; the capability-cluster features (:need:`FEAT_0081` –
:need:`FEAT_0085`) and their decisions describe the *why*.

.. building-block:: ethercat-netcfg (parse layer)
   :id: BB_0124
   :status: open
   :implements: FEAT_0081, REQ_0820, REQ_0821, REQ_0822, REQ_0823, REQ_0824, REQ_0834

   The parse crate. Turns one ``network.yaml`` document into the typed
   ``NetworkConfig`` IR — bus config, device instances, channel
   bindings — via the single public entry point
   ``pub fn parse(yaml: &str) -> Result<NetworkConfig, NetcfgError>``
   (:need:`REQ_0820`, :need:`REQ_0821`). Rejects a multi-bus document
   (:need:`REQ_0822`) and resolves channel → device references by stable
   label, never by bus address (:need:`REQ_0823`). ESI references must
   be local, vendored files; an ``http(s)://`` reference is a parse error
   (:need:`REQ_0834`), so the build never fetches. Depends on
   ``taktora-ethercat-esi`` and ``taktora-fieldbus-od-core`` (re-exporting
   ``Identity``) plus ``serde`` / ``serde_norway``; it carries no
   dependency on the connector runtime (:need:`REQ_0824`). Build-host
   tool, ``std``. Source: ``crates/taktora-ethercat-netcfg/src/``.

.. building-block:: ethercat-netcfg-codegen (codegen layer)
   :id: BB_0125
   :status: open
   :implements: FEAT_0082, REQ_0825, REQ_0826, REQ_0827, REQ_0828, REQ_0829, REQ_0836, REQ_0837, REQ_0838

   The code generator. ``pub fn generate(&NetworkConfig) -> Result<String,
   CodegenError>`` turns the IR into ``prettyplease``-formatted Rust
   source for the ``taktora-connector-ethercat`` runtime: the static
   ``SubDeviceMap`` / ``PDO_MAP`` tables (:need:`REQ_0825`), named
   ``EthercatRouting`` and channel-name constants (:need:`REQ_0826`),
   configured addresses assigned by bus position unless overridden
   (:need:`REQ_0827`), the expected working counter derived from PDO
   directions (:need:`REQ_0828`), and the bring-up identity table for
   the physical-bus assertions (:need:`REQ_0838`, codegen half). It is
   also where the derivable-fault validation lives — overlapping slices,
   out-of-image or zero-length slices, dangling labels and collisions are
   hard errors (:need:`REQ_0836`), unmapped process-image gaps warn
   (:need:`REQ_0837`) — so a bad configuration fails at ``cargo build``,
   not on the bus. Output is byte-deterministic for a given IR
   (:need:`REQ_0829`). The runtime types are named textually, so this
   crate depends on ``taktora-ethercat-netcfg``, ``proc-macro2`` /
   ``quote`` / ``syn`` / ``prettyplease`` and never on
   ``taktora-connector-ethercat``. Source:
   ``crates/taktora-ethercat-netcfg-codegen/src/``.

.. building-block:: ethercat-netcfg-build (build-script glue)
   :id: BB_0126
   :status: open
   :implements: FEAT_0083, REQ_0830, REQ_0831

   The ``build.rs`` entry point. A consumer's build script calls
   ``run(yaml_path)``, a thin wrapper over the unit-testable
   ``emit(yaml_path, out_dir) -> Result<Emitted, BuildError>`` that reads
   the YAML, drives :need:`BB_0124` and :need:`BB_0125`, writes
   ``$OUT_DIR/network.rs`` for the consumer to ``include!``
   (:need:`REQ_0830`), and prints the ``cargo:rerun-if-changed``
   directives so a config edit regenerates the module (:need:`REQ_0831`;
   today the YAML only — the per-vendored-ESI dependency is a tracked
   gap). Errors from every layer are surfaced as one ``BuildError``.
   Source: ``crates/taktora-ethercat-netcfg-build/src/``; consumers:
   ``examples/ethercat-stepper``, ``examples/ethercat-wago-coupler``.

.. building-block:: ethercat-netcfg-cli (netcfg front end)
   :id: BB_0127
   :status: open
   :implements: FEAT_0084, REQ_0832, REQ_0833, REQ_0835

   The ``netcfg`` command-line front end: a thin ``clap`` binary over a
   library core that returns ``String``s rather than printing, so every
   subcommand is unit-testable. ``netcfg expand`` prints the
   build-equivalent generated module for inspection (:need:`REQ_0832`,
   byte-identical to :need:`BB_0126`'s output); ``netcfg fetch`` vendors
   ESI files next to the config and pins them by SHA-256 and revision in
   a JSON lockfile (:need:`REQ_0833`, :need:`REQ_0835` — local sources
   today; a remote fetch path is a tracked gap); ``netcfg verify``
   re-checks the pins. Depends on the parse and codegen crates plus
   ``clap`` / ``serde_json`` / ``sha2``; never on the connector runtime.
   Source: ``crates/taktora-ethercat-netcfg-cli/src/``.

.. building-block:: ethercat-netcfg SM-watchdog resolution and validation
   :id: BB_0096
   :status: implemented
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
