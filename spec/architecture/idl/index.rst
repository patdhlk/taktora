Message-plane interface-description codegen — architecture (arc42)
==================================================================

Architecture documentation for the message-plane interface-description
codegen toolchain (see :doc:`../../requirements/idl/index`), structured
per the arc42 template and encoded with sphinx-needs using the useblocks
"x-as-code" arc42 directive types. Mirrors the structure of
:doc:`../device-codegen/index` and :doc:`../canopen-codegen/index` so
reviewers can read the device-plane and message-plane umbrellas 1:1.

Each architectural element ``:refines:`` or ``:implements:`` a parent
requirement so the trace is preserved end-to-end.

This chapter is split across pages (see the toctree): the framing
sections §1–§3 (goals, constraints, context and scope — including the
crate-dependency-graph mermaid) live here on the index; the building
blocks (§4) live in :doc:`building-blocks`; the solution strategy and its
ADRs (§5) live in :doc:`solution-strategy`; and the risks (§6) live in
:doc:`risks`.

.. contents:: Sections
   :local:
   :depth: 1

----

1. Introduction and goals
-------------------------

The toolchain's reason-to-exist is **build-time monomorphisation of
message (de)serializers**: an interface description (a CAN ``.dbc`` today)
declares the messages on a channel; we want strongly-typed
``encode``/``decode`` code per message, with a runtime path small enough
to audit and free of ``serde`` and heap allocation.

Quality goals capture the qualities the architecture is optimised for.

.. quality-goal:: Boundedness by construction
   :id: QG_0022
   :status: accepted
   :refines: FEAT_0111

   Every type the IR can express shall have a finite, compile-time-known
   serialized-length bound, and that property shall be unrepresentable to
   violate — ``String`` and ``Sequence`` carry their capacity in the type
   (:need:`REQ_0946`). The bound is what sizes downstream fixed buffers,
   so it must be a true upper bound, not a hint.

.. quality-goal:: Auditable runtime (serde-free, no_std, no-heap)
   :id: QG_0023
   :status: accepted
   :refines: FEAT_0112

   The only crate that links into a runtime consumer
   (``taktora-idl-wire``) shall be ``no_std``, allocation-free, and
   dependency-free, with no ``serde`` and no reflection
   (:need:`REQ_0950`). The (de)serialization path is kept small enough to
   stay tractable for Kani/Miri so the bytes-on-the-wire contract can be
   reasoned about formally.

.. quality-goal:: Layering integrity (strict left-to-right deps)
   :id: QG_0024
   :status: accepted
   :refines: FEAT_0110

   Each crate shall depend only on crates to its left in the
   IR → wire → frontend → codegen → backend chain (see
   :need:`ARCH_0081`). The IR knows no wire format; the codegen knows no
   wire format; only a backend does. Collapsing the layering — a frontend
   reaching for a backend, the IR naming a transport — is rejected; the
   layering is the design.

----

2. Constraints
--------------

.. constraint:: no_std + no-deps baseline for the wire runtime
   :id: CON_0028
   :status: accepted
   :refines: FEAT_0112

   ``taktora-idl-wire`` shall be ``#![no_std]`` and carry no external
   dependency (per :need:`REQ_0950`). The IR (``taktora-idl-core``) and
   the host-side codegen crates may depend on ``std``, ``serde``,
   ``proc-macro2``, and ``quote``; the runtime crate may not. This is the
   structural guarantee behind :need:`QG_0023`.

----

3. Context and scope
--------------------

.. architecture:: Toolchain layering (crate dependency graph)
   :id: ARCH_0081
   :status: implemented
   :refines: FEAT_0110

   Five layers, strict left-to-right dependency. The IR and the wire
   runtime are the two roots; a frontend lowers a description onto the
   IR, the codegen resolves the IR, and a backend joins the resolved IR
   with the frontend's physical-layout sidecar to emit code calling the
   wire runtime.

   .. mermaid::

      graph LR
        subgraph IR["1. IR core"]
          C["taktora-idl-core<br/>Module, Struct, Field,<br/>EnumDef, Service"]
        end
        subgraph Wire["2. Wire runtime"]
          W["taktora-idl-wire<br/>WireType + bit-packing<br/>(no_std, no deps)"]
        end
        subgraph Front["3. Frontend"]
          D["taktora-idl-dbc<br/>parse + lower →<br/>(Module, DbcLayout)"]
        end
        subgraph Gen["4. Codegen layer"]
          G["taktora-idl-codegen<br/>naming + MessageBackend<br/>+ resolve / generate"]
        end
        subgraph Back["5. Backend + harness"]
          B["taktora-idl-codegen-can<br/>CanBackend"]
          T["taktora-idl-codegen-can-tests<br/>build-time round-trip"]
        end
        subgraph Cons["Consumers"]
          U["generated module<br/>+ any runtime consumer"]
        end
        C --> D
        C --> G
        D --> B
        G --> B
        B --> T
        W --> T
        B -. emits code calling .-> W
        W --> U
        B --> U

.. architecture:: Build-time vs runtime separation
   :id: ARCH_0082
   :status: implemented
   :refines: FEAT_0110

   The toolchain runs entirely at build time. A runtime consumer sees
   only the generated module and links against ``taktora-idl-wire`` for
   the trait and primitives; it does not depend on ``taktora-idl-core``,
   ``taktora-idl-dbc``, ``taktora-idl-codegen``, or
   ``taktora-idl-codegen-can``. This is the structural guarantee behind
   :need:`QG_0023` and :need:`REQ_0945`.

.. toctree::
   :maxdepth: 2

   building-blocks
   solution-strategy
   risks
