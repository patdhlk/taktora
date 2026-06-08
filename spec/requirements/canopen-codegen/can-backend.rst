taktora-connector-can backend
==============================

The opinionated concrete backend (:need:`BB_0083`): emits per-device
structs implementing the runtime traits in :need:`FEAT_0065`. The only
crate in the toolchain that depends on ``canopen-eds-rt``.

.. feat:: taktora-connector-can codegen backend
   :id: FEAT_0064
   :status: open
   :satisfies: FEAT_0060

   The opinionated, concrete backend that emits per-device structs
   implementing the runtime traits in :need:`FEAT_0065`. The only
   crate in the toolchain that depends on ``canopen-eds-rt``.

.. req:: Backend crate is the sole canopen-eds-rt dependency
   :id: REQ_0740
   :status: open
   :satisfies: FEAT_0064

   ``canopen-eds-codegen-taktora`` shall be the only crate in the
   toolchain that declares ``canopen-eds-rt`` as a dependency.
   Neither ``canopen-eds``, ``canopen-eds-codegen``,
   ``canopen-eds-build``, ``canopen-eds-cli``, nor
   ``canopen-eds-verify`` shall depend on ``canopen-eds-rt``.

.. req:: One device struct per EDS file
   :id: REQ_0741
   :status: open
   :satisfies: FEAT_0064

   For each parsed EDS file (per :need:`REQ_0735`), the backend
   shall emit exactly one Rust struct named per the sanitised
   product ident (per :need:`REQ_0731` and :need:`REQ_0732`),
   deriving ``Debug + Default + Clone``.

.. req:: Identity const emitted per device
   :id: REQ_0742
   :status: open
   :satisfies: FEAT_0064

   For each generated device struct, the backend shall emit an
   accompanying ``pub const <IDENT>_IDENTITY: Identity = Identity {
   vendor_id, product_code, revision };`` so identity-driven
   dispatch (per :need:`REQ_0745`) can use a static table.

.. req:: PDO declarations emitted as sum types
   :id: REQ_0743
   :status: open
   :satisfies: FEAT_0064

   For each declared PDO mapping in the EDS, the backend shall
   emit one variant of ``<IDENT>Rpdo`` / ``<IDENT>Tpdo`` enum plus
   one payload struct per variant. Modelling PDOs as nullable
   fields on the device struct is rejected — every declared PDO is
   a closed, named choice.

.. req:: Dummy entries skipped in PDO payload structs
   :id: REQ_0744
   :status: open
   :satisfies: FEAT_0064

   For each PDO mapping, the backend shall skip CANopen ``Dummy*``
   data-type entries when emitting payload struct fields — only
   real mapped objects appear as fields. Bit offsets of real
   fields shall be threaded through generated ``decode`` /
   ``encode`` bodies (not carried as named padding fields). See
   :need:`ADR_0083`.

.. req:: Generated module root exposes a registry
   :id: REQ_0745
   :status: open
   :satisfies: FEAT_0064

   The module root emitted by ``emit_module_root`` shall expose a
   ``registry!()`` declarative macro (or equivalent generated
   ``static`` table) mapping each emitted device's ``Identity`` to
   a factory closure returning ``Box<dyn CanOpenDevice>``.
   Identity-based dispatch in a downstream adapter shall be
   reducible to a ``HashMap`` lookup against this table.

.. req:: Bring-up SDO writes emitted from EDS
   :id: REQ_0746
   :status: open
   :satisfies: FEAT_0064

   ``impl CanOpenConfigurable for <IDENT>`` shall emit SDO writes
   for: PDO communication parameters (OD ranges
   ``0x1400..0x14FF`` and ``0x1800..0x18FF``), PDO mapping
   parameters (``0x1600..0x17FF`` and ``0x1A00..0x1BFF``), and the
   ``[DeviceInfo].NMT_BootSlave``-driven NMT start request when
   declared. Values shall come from the EDS — per-bus customisation
   is DCF territory, out of scope per :need:`REQ_0790`.

.. req:: Object dictionary emission is a default-off cargo feature
   :id: REQ_0747
   :status: open
   :satisfies: FEAT_0064

   Emission of the full OD table per device (as a sorted
   ``static OD: &[(u16, u8, DataType, &str)]`` lookup, per
   :need:`ADR_0075`) shall be gated behind a default-off
   ``object-dictionary`` cargo feature on the generated module's
   parent crate. PDOs and bring-up SDO writes shall remain
   unconditional; only the OD table is gated.

.. req:: Generated code compiles under no_std + alloc
   :id: REQ_0748
   :status: open
   :satisfies: FEAT_0064

   The emitted device modules shall compile under ``#![no_std]`` +
   ``alloc``. The backend shall not emit ``std::``-qualified paths
   in generated code.
