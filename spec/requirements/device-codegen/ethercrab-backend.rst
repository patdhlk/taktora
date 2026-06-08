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
   :status: open
   :satisfies: FEAT_0053

   For each ``<Device>`` element parsed from the input ESI files,
   the backend shall emit exactly one Rust struct named per the
   sanitised product ident (per :need:`REQ_0511` and
   :need:`REQ_0512`), deriving ``Debug + Default + Clone``.

.. req:: Identity const emitted per device
   :id: REQ_0522
   :status: open
   :satisfies: FEAT_0053

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

.. req:: PDO assignment alternatives emitted as sum type
   :id: REQ_0523
   :status: open
   :satisfies: FEAT_0053

   When an ESI device declares multiple PDO assignment alternatives
   (typically "Standard" / "Compact"), the backend shall emit a
   ``<IDENT>PdoAssignment`` enum with one variant per alternative.
   Modelling alternatives with ``Option<…>`` fields on the device
   struct is rejected — every alternative is a closed, named choice.

.. req:: One PDO struct per assignment alternative
   :id: REQ_0524
   :status: open
   :satisfies: FEAT_0053

   For each variant of ``<IDENT>PdoAssignment``, the backend shall
   emit a corresponding ``<IDENT>Pdo<Variant>`` struct that holds
   the typed PDO entries for that variant. The device struct's
   ``pdo`` field shall be a sum type whose variants embed these
   per-alternative structs.

.. req:: Generated module root exposes a registry
   :id: REQ_0525
   :status: open
   :satisfies: FEAT_0053

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
