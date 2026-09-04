ethercrab codegen backend
=========================

The one concrete backend (:need:`BB_0062`) — the only crate in the
toolchain that depends on ``ethercrab``. Emits per-device structs that
implement the runtime traits of :doc:`runtime-trait`.

.. feat:: ethercrab codegen backend
   :id: FEAT_0053
   :status: open
   :satisfies: FEAT_0050

   The opinionated, concrete backend that emits per-device structs
   implementing the runtime traits in :need:`FEAT_0054`. This is the
   only crate in the toolchain that depends on ``ethercrab``.

.. req:: Backend crate is the sole ethercrab dependency
   :id: REQ_0520
   :status: open
   :satisfies: FEAT_0053

   ``ethercat-esi-codegen-ethercrab`` shall be the only crate in the
   toolchain that declares ``ethercrab`` (any version) as a
   dependency. Neither ``ethercat-esi``, ``ethercat-esi-codegen``,
   ``ethercat-esi-build``, nor ``ethercat-esi-verify`` shall depend
   on ``ethercrab``.

.. req:: One device struct per ESI device entry
   :id: REQ_0521
   :status: implemented
   :satisfies: FEAT_0053
   :links: BB_0062, TEST_0420

   For each ``<Device>`` element parsed from the input ESI files,
   the backend shall emit exactly one Rust struct named per the
   sanitised product ident (per :need:`REQ_0511` and
   :need:`REQ_0512`), deriving ``Debug + Default + Clone``.

.. req:: Identity const emitted per device
   :id: REQ_0522
   :status: implemented
   :satisfies: FEAT_0053
   :links: BB_0062, TEST_0420

   For each generated device struct, the backend shall emit an
   accompanying ``pub const <IDENT>_REV<REV>: Identity =
   Identity { vendor_id, product_code, revision };`` so
   identity-driven dispatch (per :need:`REQ_0525`) can use a static
   table. ``Identity`` is the shared ``taktora-fieldbus-od-core``
   type (:need:`ADR_0078`); the toolchain does not mint a separate
   ``SubDeviceIdentity``. Mapping this triple onto ethercrab's
   wire-read identity (which additionally carries ``serial``) is the
   connector adapter's concern (:need:`BB_0067`), not the generated
   code's.

.. req:: Selectable PDO assignments emitted as a joint per-device OpMode enum
   :id: REQ_0523
   :status: implemented
   :satisfies: FEAT_0053
   :links: BB_0062, TEST_0870

   The backend shall resolve each device to a set of selectable PDO
   assignments and emit one joint ``<Dev>OpMode`` enum carrying a
   single variant per resolved assignment — one per
   ``<AlternativeSmMapping>`` (per :need:`REQ_0529`), or a single
   ``Default`` variant when the device declares none. The variant set
   spans both directions at once: a variant is a complete operating
   mode, not a per-direction choice. The device itself shall be emitted
   as ``struct <Dev> { mode: <Dev>OpMode }``.

   Modelling assignments with ``Option<…>`` fields on the device struct
   is rejected — every operating mode is a closed, named choice
   (:need:`ADR_0104`, refining :need:`ADR_0072`). The earlier
   per-direction ``<IDENT>PdoAssignment`` sum type, which could not
   represent a multi-PDO assignment nor pair an RxPDO set with its
   TxPDO set, is retired in favour of this joint enum.

.. req:: Each OpMode variant carries a per-mode inputs/outputs data struct
   :id: REQ_0524
   :status: implemented
   :satisfies: FEAT_0053
   :links: BB_0062, TEST_0870

   Each ``<Dev>OpMode`` variant shall carry a ``{ inputs, outputs }``
   payload holding the typed process data for that mode. Each direction
   shall be flat-or-per-PDO: a single typed struct when the active mode
   assigns one PDO to that direction, or a struct composed of the
   per-PDO entry structs when it assigns several. Decode, encode, and
   the ``input_len`` / ``output_len`` lengths shall be computed per the
   active variant — the lengths of the variant currently held in
   ``mode`` — so a device's process-image size follows its selected
   operating mode.

.. req:: Generated module root exposes a registry
   :id: REQ_0525
   :status: implemented
   :satisfies: FEAT_0053
   :links: BB_0062, TEST_0421

   The module root emitted by ``emit_module_root`` shall expose a
   ``registry!()`` declarative macro (or equivalent generated
   ``static`` table) that maps each emitted device's
   ``Identity`` to a factory closure returning
   ``Box<dyn EsiDevice>``. Identity-based dispatch in downstream
   code (e.g. ``taktora-connector-ethercat``) shall be reducible to a
   ``HashMap`` lookup against this table.

.. req:: Generated code compiles under no_std + alloc
   :id: REQ_0526
   :status: open
   :satisfies: FEAT_0053

   The emitted device modules shall compile under ``#![no_std]`` +
   ``alloc`` so generated drivers are usable from embedded contexts.
   The backend shall not emit ``std::``-qualified paths in
   generated code.

.. req:: Default PDO assignment derived from Sm/Mandatory, not Fixed
   :id: REQ_0527
   :status: implemented
   :satisfies: FEAT_0053
   :links: BB_0062, TEST_0870

   When a device declares no ``<AlternativeSmMapping>`` (its
   ``Default`` mode, per :need:`REQ_0523`), the backend shall derive
   that mode's PDO assignment from the per-PDO ``Sm=`` attribute and the
   ``Mandatory`` flag — i.e. the PDOs the ESI assigns to a sync manager
   by default. The Beckhoff ``Fixed="1"`` attribute shall **not** be
   read as "always-on" / "always-assigned": ``Fixed`` is orthogonal to
   assignment and means only that the PDO's *entry list is not editable*
   (the wire layout is locked). A ``Fixed`` PDO that the default mapping
   does not assign to a sync manager is not part of the default
   assignment. This decoupling is :need:`ADR_0104`. issue #70.

.. req:: Per-active-mode Rx/Tx PDO-index lists exposed for 0x1C12/0x1C13
   :id: REQ_0528
   :status: implemented
   :satisfies: FEAT_0053
   :links: BB_0062, TEST_0870

   Each generated device shall expose, via an inherent
   ``fn pdo_assignment(&self) -> PdoAssignment`` (not a trait method),
   the Rx and Tx PDO **index** lists of the currently-held
   ``mode`` (per :need:`REQ_0523`) so a connector can program the SM
   PDO-assignment objects ``0x1C12`` (RxPDO / SM2) and ``0x1C13``
   (TxPDO / SM3). The indices shall be surfaced as plain ``u16`` slices
   (``&[u16]``) — the raw PDO indices, not typed handles. The runtime
   ``EsiDevice`` trait surface (:need:`ADR_0098`) is unchanged; this is
   an inherent method on the concrete device struct, available to
   identity-dispatched bring-up code without widening the object-safe
   trait. issue #70.
