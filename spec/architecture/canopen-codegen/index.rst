CANopen device-driver codegen — architecture (arc42)
====================================================

Architecture documentation for the CANopen device-driver codegen
toolchain (see :doc:`../../requirements/canopen-codegen/index`), structured
per the arc42 template and encoded with sphinx-needs using the useblocks "x-as-code"
arc42 directive types. Mirrors the structure of
:doc:`../device-codegen/index` so reviewers can read both umbrellas 1:1.

Each architectural element ``:refines:`` or ``:implements:`` a parent
requirement so the trace is preserved end-to-end.

This chapter is split across pages (see the toctree): the framing
sections §1–§3 (goals, constraints, context and scope — including the
crate-dependency-graph mermaid) live here on the index; the building
blocks (§4) live in :doc:`building-blocks`; the solution strategy and
its ADRs (§5) live in :doc:`solution-strategy`; the risks (§6) live in
:doc:`risks`; and the cross-cutting traceability (§7) lives in
:doc:`cross-cutting`.

.. contents:: Sections
   :local:
   :depth: 1

----

1. Introduction and goals
-------------------------

The toolchain's reason-to-exist is **build-time monomorphisation of
CANopen device drivers**: vendor-supplied EDS files describe each
node's PDOs, OD, and bring-up SDO sequence; we want strongly-typed
``on_rpdo`` / ``drain_tpdos`` code per device, with zero INI parsing
at runtime and no hand-written boilerplate per node.

Quality goals capture the qualities the architecture is optimised for.

.. quality-goal:: Build-time determinism (same EDS in → same code out)
   :id: QG_0014
   :status: open
   :refines: FEAT_0060

   The same set of EDS inputs (modulo file ordering) shall produce a
   byte-identical generated module across machines, toolchain
   versions, and clock walls. Generation order, hash-map iteration
   order, and timestamp inclusion are explicitly excluded as sources
   of nondeterminism. Required so the generated file is reviewable
   in diffs and cachable in CI.

.. quality-goal:: Layering integrity (strict left-to-right deps)
   :id: QG_0015
   :status: open
   :refines: FEAT_0060

   Each crate in the toolchain shall depend only on crates to its
   left in the OD-core → parse → codegen → tooling chain (see
   :need:`ARCH_0070`). Crossover dependencies — e.g. the parser
   reaching for ``canopen-eds-rt`` types, the build helper bypassing
   the codegen layer — are rejected. The layering is the design;
   collapsing it deletes its value.

.. quality-goal:: Zero runtime cost of codegen presence
   :id: QG_0016
   :status: open
   :refines: FEAT_0060

   A consumer of the generated modules shall pay no runtime cost
   for the codegen layer's existence: no INI parse, no allocation
   for OD tables when ``object-dictionary`` is off, no ``Box<dyn>``
   indirection on the frame hot path, no string IDs at runtime.

.. quality-goal:: Trait stability for ecosystem adoption
   :id: QG_0017
   :status: open
   :refines: FEAT_0065

   The ``CanOpenDevice`` / ``CanOpenConfigurable`` pair (per
   :need:`REQ_0750` / :need:`REQ_0751`) is the contract any CAN
   consumer would pivot on if generated drivers became
   community-shared. Breaking changes shall be rare and
   well-publicised; additive evolution (default methods, new
   associated types with defaults) is preferred over rewrites.

----

2. Constraints
--------------

.. constraint:: cargo build-script semantics
   :id: CON_0020
   :status: open
   :refines: FEAT_0066

   The build helper shall live within cargo's ``build.rs``
   contract: no network access, no writes outside ``OUT_DIR``,
   ``cargo:rerun-if-changed`` directives for each input file
   (per :need:`REQ_0762`), and no reliance on cargo features being
   on or off in a way that varies across consumers.

.. constraint:: CiA 301 / 306 own the EDS schema
   :id: CON_0021
   :status: open
   :refines: FEAT_0062

   The EDS schema is published by CAN in Automation (CiA 306) and
   the underlying OD semantics by CiA 301. The parser shall track
   the published schema; schema drift across vendors shall be
   handled as captured in :need:`REQ_0724` (unknown-section policy)
   and :need:`REQ_0725` (liberal-quirks policy) rather than by
   hard-failing the parse.

.. constraint:: no_std + alloc baseline for OD core, parser, runtime
   :id: CON_0022
   :status: open
   :refines: FEAT_0061

   ``fieldbus-od-core``, ``canopen-eds``, and ``canopen-eds-rt``
   shall be ``#![no_std]`` with an ``alloc`` dependency (per
   :need:`REQ_0701` / :need:`REQ_0721` / :need:`REQ_0748`). Build
   helpers, CLI, and verifier may depend on ``std``.

.. constraint:: heapless 0.8 surface for fixed-capacity buffers
   :id: CON_0023
   :status: open
   :refines: FEAT_0065

   ``PdoOut::payload`` shall be ``heapless::Vec<u8, 8>`` from the
   ``heapless`` 0.8 family (or whichever version taktora's workspace
   pins). The constant-8 capacity matches classical CAN's 8-byte
   payload cap; CAN-FD's 64-byte payload is deferred per
   :need:`REQ_0791` and :need:`ADR_0084`.

----

3. Context and scope
--------------------

.. architecture:: Toolchain layering (crate dependency graph)
   :id: ARCH_0070
   :status: open
   :refines: FEAT_0060

   Five layers, strict left-to-right dependency. Each crate has one
   job and depends only on crates to its left. The follow-on
   ``taktora-connector-can`` adapter would sit to the right of the
   runtime trait crate and is unaware of INI or codegen.

   .. mermaid::

      graph LR
        subgraph OD["1. Shared OD core"]
          ODC["fieldbus-od-core<br/>Identity, DictEntry,<br/>PdoEntry, PdoMap"]
        end
        subgraph Parse["2. Parse layer"]
          EE["ethercat-esi<br/>(re-exports + ESI specifics)"]
          P["canopen-eds<br/>INI → typed IR"]
        end
        subgraph Gen["3. Codegen layer"]
          G["canopen-eds-codegen<br/>IR → TokenStream"]
          B["canopen-eds-codegen-taktora<br/>concrete backend"]
        end
        subgraph RT["4. Runtime trait"]
          RTC["canopen-eds-rt<br/>CanOpenDevice / CanOpenConfigurable"]
        end
        subgraph Tool["5. Tooling layer"]
          BR["canopen-eds-build<br/>build.rs glue"]
          CLI["canopen-eds-cli<br/>cargo eds expand / list"]
          VER["canopen-eds-verify<br/>EDS ↔ SDO-dump diff"]
        end
        subgraph Cons["Consumers (follow-on)"]
          USER["any CAN consumer<br/>(includes generated code)"]
          SCC["taktora-connector-can<br/>thin CanOpenDevice adapter"]
        end
        ODC --> EE
        ODC --> P
        P --> G
        G --> B
        B --> RTC
        B --> BR
        B --> CLI
        P --> VER
        RTC --> BR
        BR --> USER
        USER --> SCC

.. architecture:: Build-time vs runtime separation
   :id: ARCH_0071
   :status: open
   :refines: FEAT_0060

   The toolchain runs entirely at build time. Runtime consumers
   only see the generated module and link against
   ``canopen-eds-rt`` for the trait definitions; they do not depend
   on ``canopen-eds``, ``canopen-eds-codegen``, or any tooling
   crate. This is the structural guarantee behind :need:`QG_0016`
   and :need:`REQ_0794`.

.. toctree::
   :maxdepth: 2

   building-blocks
   solution-strategy
   risks
   cross-cutting
