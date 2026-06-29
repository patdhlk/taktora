Solution strategy and decisions
===============================

arc42 §5 — the architecture decisions that shape the toolchain. Each
``arch-decision`` ``:refines:`` the requirement or feature it serves.

.. arch-decision:: Two planes: a message-plane IR mirroring the device plane
   :id: ADR_0117
   :status: accepted
   :refines: FEAT_0110

   **Context.** taktora already has a device-plane IR
   (``fieldbus-od-core``) describing a device's identity, object
   dictionary, and cyclic process image. Messages crossing a channel
   (CAN signals today, OMG IDL / ROS 2 later) are a different shape —
   structs, enums, services — but share the same need for a bounded,
   policy-free IR onto which several frontends lower.

   **Decision.** Introduce ``taktora-idl-core`` as the message-plane
   *twin* of ``fieldbus-od-core``: a separate crate, same architectural
   role, deliberately not unified with the device-plane IR. The two
   planes describe different things and should not be forced into one
   type family.

   **Consequences.** ✅ Each plane evolves on its own axis; a CAN signal
   model does not leak into a device's object dictionary. ✅ Frontends and
   backends compose per plane. ❌ Two IR crates to learn — mitigated by
   their parallel structure, so a reader who knows one reads the other.

.. arch-decision:: Split the serde-free wire runtime from host-side codegen
   :id: ADR_0118
   :status: accepted
   :refines: FEAT_0112

   **Context.** Code emission needs ``proc-macro2``, ``quote``, and host
   conveniences. The (de)serialization path that ships in a product is
   safety-relevant and wants the opposite: ``no_std``, no heap, no
   ``serde``, no reflection — small enough to reason about with Kani/Miri.

   **Decision.** Put the runtime contract — the ``WireType`` trait and
   the bit-packing primitives — in its own dependency-free ``no_std``
   crate (``taktora-idl-wire``). Generated code calls only that crate.
   Naming and emission policy stay host-side in ``taktora-idl-codegen``.

   **Consequences.** ✅ A consumer links a tiny, auditable crate, not the
   codegen toolchain. ✅ The encode/decode contract is verifiable in
   isolation. ❌ The generated code and the runtime crate must agree on a
   stable primitive surface — a deliberate, small API boundary.

.. arch-decision:: DBC as the first frontend (bounded by construction)
   :id: ADR_0119
   :status: accepted
   :refines: FEAT_0113

   **Context.** The IR's central invariant is boundedness. OMG IDL and
   ROS 2 ``.msg`` admit unbounded sequences, so a frontend for them must
   actively reject the unbounded case — useful, but it conflates "does
   the pipeline work" with "does rejection work".

   **Decision.** Ship DBC first. Every DBC message has a fixed DLC and
   every signal a fixed bit width, so lowering a ``.dbc`` cannot produce
   an unbounded type; the boundedness invariant is never even at risk.
   DBC proves the description → IR → codegen pipeline end-to-end before a
   harder frontend introduces real rejection logic.

   **Consequences.** ✅ The first proof-of-life is the simplest possible.
   ✅ The unbounded-rejection path is added later without reworking the
   pipeline. ❌ The rejection path is unexercised until a second frontend
   lands (tracked as :need:`RISK_0024`).

.. arch-decision:: Plane-generic codegen with a backend trait; CAN is one backend
   :id: ADR_0120
   :status: accepted
   :refines: FEAT_0114

   **Context.** Naming policy, identifier collision detection, and field
   classification are wire-format-independent; bit-packing a CAN frame is
   not. Folding both into one crate would force every future backend
   (CDR, a minimal codec) to recompile the CAN opinion.

   **Decision.** ``taktora-idl-codegen`` owns the plane-generic half —
   ``resolve`` + naming + the ``MessageBackend`` trait + ``generate`` —
   and knows no wire format. ``taktora-idl-codegen-can`` is one concrete
   ``MessageBackend`` that adds CAN bit-packing, joining the resolved IR
   with the DBC layout sidecar.

   **Consequences.** ✅ New backends are net-additive — no churn in the
   IR, naming, or resolve layers. ✅ The CAN backend is small: it adds
   only the wire format. ❌ One more crate boundary than a monolith —
   accepted, matching the device-plane toolchain's layering.
