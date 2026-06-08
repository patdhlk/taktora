Device-driver codegen — architecture (arc42)
============================================

Architecture documentation for the device-driver codegen toolchain
(see :doc:`../../requirements/device-codegen/index`), structured per the arc42
template and encoded with sphinx-needs using the useblocks "x-as-code"
arc42 conventions.

Each architectural element ``:refines:`` or ``:implements:`` a parent
requirement so the trace is preserved end-to-end.

This chapter is split across pages (see the toctree): the framing
sections §1–§3 (goals, constraints, context and scope — including the
crate-dependency-graph mermaid) live here on the index; the solution
strategy and its ADRs (§4) live in :doc:`solution-strategy`; the
building-block decomposition (§5) lives in :doc:`building-blocks`; the
runtime build-time and bring-up sequences (§6) live in
:doc:`runtime-view`; the crosscutting tail (§7 deployment, §8
crosscutting concepts, §10 quality requirements, §12 glossary) lives in
:doc:`crosscutting`; the decisions pointer (§9) lives in
:doc:`decisions`; and the risks and technical debt (§11) live in
:doc:`risks`.

.. contents:: Sections
   :local:
   :depth: 1

----

1. Introduction and goals
-------------------------

The toolchain's reason-to-exist is **build-time monomorphisation of
fieldbus device drivers**: vendor-supplied ESI XML files describe each
EtherCAT device's PDOs, mailbox, and OD; we want strongly-typed
``decode_inputs`` / ``encode_outputs`` code per device, with zero XML
at runtime and no hand-written boilerplate per terminal.

Quality goals capture the qualities the architecture is optimised for.

.. quality-goal:: Build-time determinism (same ESI in → same code out)
   :id: QG_0010
   :status: open
   :refines: FEAT_0050

   The same set of ESI inputs (modulo file ordering) shall produce a
   byte-identical generated module across machines, toolchain
   versions, and clock walls. Generation order, hash-map iteration
   order, and timestamp inclusion are explicitly excluded as sources
   of nondeterminism. Required so the generated file is reviewable
   in diffs and cachable in CI.

.. quality-goal:: Layering integrity (strict left-to-right deps)
   :id: QG_0011
   :status: open
   :refines: FEAT_0050

   Each crate in the toolchain shall depend only on crates to its
   left in the parse → codegen → tooling chain (see
   :need:`ARCH_0050`). Crossover dependencies — e.g. the parser
   reaching for ethercrab types, the build helper bypassing the
   codegen layer — are rejected. The layering is the design;
   collapsing it deletes its value.

.. quality-goal:: Zero runtime cost of codegen presence
   :id: QG_0012
   :status: open
   :refines: FEAT_0050

   A consumer of the generated modules shall pay no runtime cost
   for the codegen layer's existence: no XML parse, no allocation
   for OD tables when ``object-dictionary`` is off, no ``Box<dyn>``
   indirection on the cyclic hot path, no string IDs at runtime.

.. quality-goal:: Trait stability for ecosystem adoption
   :id: QG_0013
   :status: open
   :refines: FEAT_0054

   The ``EsiDevice`` / ``EsiConfigurable`` pair (per
   :need:`REQ_0530` / :need:`REQ_0531`) is the contract any
   ethercrab user would pivot on if generated drivers became
   community-shared. Breaking changes shall be rare and
   well-publicised; additive evolution (default methods, new
   associated types with defaults) is preferred over rewrites.

----

2. Constraints
--------------

.. constraint:: cargo build-script semantics
   :id: CON_0010
   :status: open
   :refines: FEAT_0055

   The build helper shall live within cargo's ``build.rs``
   contract: no network access, no writes outside ``OUT_DIR``,
   ``cargo:rerun-if-changed`` directives for each input file, and
   no reliance on cargo features being on or off in a way that
   varies across consumers (per :need:`REQ_0542`).

.. constraint:: ethercrab API surface as upstream
   :id: CON_0011
   :status: open
   :refines: FEAT_0053

   The ethercrab backend shall target the ethercrab API exposed by
   ``ethercrab = "0.7"`` (or whichever version taktora-connector-ethercat
   pins, per :need:`BB_0030`-style pinning). ``SubDevicePreOperational``,
   ``SubDeviceIdentity``, and the SDO write helpers are the
   contact surface; the backend shall not depend on private types.

.. constraint:: bitvec for process-image access
   :id: CON_0012
   :status: open
   :refines: FEAT_0054

   Generated ``decode_inputs`` / ``encode_outputs`` shall operate
   on ``bitvec::slice::BitSlice<u8, Lsb0>``, matching the
   :need:`REQ_0326` / :need:`REQ_0327` PDI access pattern already
   in use by ``taktora-connector-ethercat``. A second bit-slice
   abstraction shall not be introduced.

.. constraint:: no_std + alloc baseline for parser and runtime trait
   :id: CON_0013
   :status: open
   :refines: FEAT_0051

   ``ethercat-esi`` and ``ethercat-esi-rt`` shall be ``#![no_std]``
   with an ``alloc`` dependency. Build helpers, CLI, and verifier
   may depend on ``std``.

   **Scope narrowed (2026-05).** The parser (``taktora-ethercat-esi``)
   and the shared ``taktora-fieldbus-od-core`` crate now adopt a ``std``
   baseline (see :need:`ADR_0097`); :need:`REQ_0501` is rejected. This
   constraint now governs the runtime-trait crate (:need:`FEAT_0054`)
   only, revisited in its own round.

.. constraint:: ETG owns the ESI XML schema
   :id: CON_0014
   :status: open
   :refines: FEAT_0051

   The ESI schema (``EtherCATInfo.xsd``) is published by the
   EtherCAT Technology Group, not by this project. The parser
   shall track the published schema; schema drift across vendors
   shall be handled as captured in :need:`ADR_0074` (opaque-blob
   policy for unknown elements) rather than by hard-failing the
   parse.

----

3. Context and scope
--------------------

.. architecture:: Toolchain layering (crate dependency graph)
   :id: ARCH_0050
   :status: open
   :refines: FEAT_0050

   Four layers, strict left-to-right dependency. Each crate has one
   job and depends only on crates to its left. The
   ``taktora-connector-ethercat`` consumer (:need:`FEAT_0041`) sits
   to the right of the runtime trait crate and is unaware of XML
   or codegen.

   .. mermaid::

      graph LR
        subgraph Parse["1. Parse layer"]
          P["ethercat-esi<br/>XML → typed IR"]
        end
        subgraph Gen["2. Codegen layer"]
          G["ethercat-esi-codegen<br/>IR → TokenStream"]
          B["ethercat-esi-codegen-ethercrab<br/>concrete backend"]
        end
        subgraph RT["3. Runtime trait"]
          RTC["ethercat-esi-rt<br/>EsiDevice / EsiConfigurable"]
        end
        subgraph Tool["4. Tooling layer"]
          BR["ethercat-esi-build<br/>build.rs glue"]
          CLI["ethercat-esi-cli<br/>cargo esi expand / list"]
          VER["ethercat-esi-verify<br/>diff ESI vs SII .bin"]
        end
        subgraph Cons["Consumers"]
          USER["any ethercrab user<br/>(includes generated code)"]
          SCE["taktora-connector-ethercat<br/>thin EsiDevice adapter"]
        end
        P --> G
        G --> B
        B --> RTC
        B --> BR
        B --> CLI
        P --> VER
        RTC --> BR
        BR --> USER
        USER --> SCE

.. architecture:: Build-time vs runtime separation
   :id: ARCH_0051
   :status: open
   :refines: FEAT_0050

   The toolchain runs entirely at build time. Runtime consumers
   only see the generated module and link against
   ``ethercat-esi-rt`` for the trait definitions; they do not
   depend on ``ethercat-esi``, ``ethercat-esi-codegen``, or any
   tooling crate. This is the structural guarantee behind
   :need:`QG_0012` and :need:`REQ_0593`.

.. toctree::
   :maxdepth: 2

   solution-strategy
   building-blocks
   runtime-view
   crosscutting
   decisions
   risks
