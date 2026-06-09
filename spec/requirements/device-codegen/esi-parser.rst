ESI parser
==========

The pure parse layer (:need:`BB_0060`): ESI XML in, a typed in-memory IR
out, with no knowledge of codegen, ethercrab, or taktora-executor.

.. feat:: ESI parser
   :id: FEAT_0051
   :status: implemented
   :satisfies: FEAT_0050

   A pure parser crate. Reads ESI XML, emits a typed in-memory IR.
   Knows nothing about codegen, ethercrab, or taktora-executor. Suitable
   for any downstream tool — codegen, network configurator, simulator,
   verifier.

.. req:: Pure parse function with no I/O
   :id: REQ_0500
   :status: implemented
   :satisfies: FEAT_0051
   :links: BB_0060, TEST_0400

   The crate shall expose ``parse(xml: &str) -> Result<EsiFile,
   EsiError>``. The function shall perform no filesystem or network
   I/O; the caller is responsible for reading the XML bytes.

.. req:: no_std + alloc compatible
   :id: REQ_0501
   :status: rejected
   :satisfies: FEAT_0051

   The crate shall be ``#![no_std]`` with an ``alloc`` dependency so
   it can run inside ``proc-macro``, ``build.rs``, embedded build
   tooling, or a hosted CLI without pulling in a default-features
   ``std`` surface.

   **Superseded (2026-05): std/POSIX baseline.** The parser cluster now
   targets ``std`` — taktora targets a POSIX OS today, and the ``no_std``
   constraint forced a hand-rolled error type (no ``core::error::Error``)
   and blocked ``thiserror``-derived errors carrying source positions
   (:need:`REQ_0506`). See :need:`ADR_0097`. ``no_std`` support is
   deferred, not abandoned; the runtime-trait crate (:need:`FEAT_0054`)
   posture is revisited in its own round.

.. req:: quick-xml + serde backend
   :id: REQ_0502
   :status: implemented
   :satisfies: FEAT_0051
   :links: BB_0060, TEST_0400

   The crate shall implement parsing on top of ``quick-xml`` with
   ``serde`` deserialisation. Hand-written ``Read``-based parsing is
   rejected — schema maintenance lives in the ``serde`` derives.

.. req:: Parser does not depend on ethercrab or codegen
   :id: REQ_0503
   :status: implemented
   :satisfies: FEAT_0051
   :links: BB_0060, TEST_0402

   The ``ethercat-esi`` crate shall not declare ``ethercrab`` or
   any codegen crate as a dependency. A downstream tool that only
   needs the IR shall not be forced to compile the codegen layer.

.. req:: IR carries identity, PDO maps, mailbox, DC, and OD
   :id: REQ_0504
   :status: implemented
   :satisfies: FEAT_0051
   :links: BB_0060, TEST_0400

   The IR shall represent, per device: ``Identity`` (vendor id,
   product code, revision), ``Vec<SyncManager>``, ``Vec<Pdo>`` for
   TxPDOs and RxPDOs, ``Option<Mailbox>`` capturing CoE/EoE/FoE
   support and InitCmds, ``Option<DistributedClock>``, and a
   ``Vec<DictEntry>`` for the object dictionary. The OD field shall
   be present in the IR unconditionally so non-codegen consumers can
   inspect it; codegen-side emission of OD tables is feature-gated
   per :need:`REQ_0533`.

   PDOs are captured structurally per declared ``<TxPdo>`` /
   ``<RxPdo>`` element — including PDO assignment alternatives and
   padding entries — and the parser does not resolve a flattened
   process image (no bit offsets are stored in the IR). ``Identity``,
   ``DataType``, and ``DictEntry`` are provided by the shared
   ``taktora-fieldbus-od-core`` crate and re-exported (the OD-core
   lift of :need:`ADR_0078`). Implemented in
   ``crates/taktora-ethercat-esi/src/model.rs``; verified by
   ``tests/identity.rs``, ``pdos.rs``, ``sync_managers.rs``,
   ``mailbox.rs``, and ``dc_and_od.rs``.

.. req:: ESI model exposes per-SM watchdog-trigger enable
   :id: REQ_0843
   :status: implemented
   :satisfies: FEAT_0051
   :links: BB_0060, TEST_0859

   Each ``SyncManager`` in the IR shall expose a
   ``watchdog_trigger_enable: bool`` decoded from control-byte bit 6
   (``0x40``) — the ETG SM-control watchdog-trigger-enable bit. The raw
   ``control_byte`` shall be retained alongside it. An ``<Sm>`` with no
   ``ControlByte`` attribute shall decode to ``false``: the parser shall
   never fabricate an enabled watchdog from an absent control byte.

   This is the per-SM ENABLE only. The watchdog **timeout** lives in the
   device registers ``0x0400`` / ``0x0420``, which real ESI files do not
   declare (verified against vendor files); the timeout is master-side
   configuration (``REQ_0846`` territory), not part of this faithful
   IR.

   **Rationale.** :need:`AOU_0016` requires every fieldbus output slave
   to run its sync-manager process-data watchdog **enabled** and bounded:
   under the :need:`ADR_0065` fail-fast model, on a framework-invariant
   abort the master stops emitting process-data frames and runs no
   destructors, so the slave watchdog is the **sole** mechanism that
   drives outputs to their safe state. The per-SM enable bit is the
   statically-checkable half of that assumption — the only half an ESI
   file declares — so surfacing it in the IR lets a config-time check
   detect an output SM that ships with its watchdog trigger disabled. The
   timeout bound (the other half of :need:`AOU_0016`) is not declared in
   ESI and is enforced master-side. Implemented in
   ``crates/taktora-ethercat-esi/src/model.rs`` and
   ``src/dto.rs``; verified by ``tests/sm_watchdog.rs``.

.. req:: FMMU declarations captured in the IR
   :id: REQ_0848
   :status: implemented
   :satisfies: FEAT_0051
   :links: BB_0060, TEST_0865

   The IR shall expose, per device, the ``<Fmmu>`` declarations in
   declaration order as ``Vec<Fmmu>``. The declared usage shall be
   decoded to ``FmmuUsage`` (``Inputs`` / ``Outputs`` / ``MBoxState``);
   an unrecognised usage string shall be preserved verbatim as
   ``FmmuUsage::Other`` — the parser shall not fail on it.

   **Rationale.** FMMU declarations pair with the sync managers of
   :need:`REQ_0504`; a network configurator needs them to sanity-check
   SM/FMMU pairing. The tolerate-unknown posture matches
   :need:`REQ_0505`. Implemented in
   ``crates/taktora-ethercat-esi/src/model.rs`` and ``src/dto.rs``;
   verified by ``tests/fmmu.rs``.

.. req:: EEPROM (SII source) data captured without interpretation
   :id: REQ_0849
   :status: implemented
   :satisfies: FEAT_0051
   :links: BB_0060, TEST_0866

   The IR shall expose, per device, the ``<Eeprom>`` content as
   ``Option<Eeprom>``: ``ByteSize``, the ``ConfigData`` payload, and the
   optional ``BootStrap`` payload. Hex payloads shall be decoded to raw
   bytes; a malformed hex string shall raise a semantic error carrying
   the offending element path (:need:`REQ_0506`). Unrecognised
   ``<Eeprom>`` children (e.g. ``<Category>`` blocks) shall be retained
   verbatim as ``RawXml``.

   The parser shall perform **no SII interpretation**: PDI fields,
   checksums, and SII image assembly are consumer work
   (:need:`FEAT_0057`, :need:`ADR_0102`).

   **Rationale.** EEPROM-image verification (:need:`FEAT_0057`) needs
   the declared SII source data; decoding hex to bytes is a lossless
   re-representation, while interpreting it would bake verifier policy
   into the parser. Implemented in
   ``crates/taktora-ethercat-esi/src/model.rs``, ``src/dto.rs``, and
   ``src/raw_xml.rs``; verified by ``tests/eeprom.rs``.

.. req:: MDP module catalog and slot constraints captured, never resolved
   :id: REQ_0850
   :status: implemented
   :satisfies: FEAT_0051
   :links: BB_0060, TEST_0867

   For modular devices (MDP), the IR shall expose the file-level
   ``<Modules>`` catalog as ``Vec<Module>`` — each module carrying its
   ``ModuleIdent``, type string, name, and per-module ``TxPdo`` /
   ``RxPdo`` lists (reusing the :need:`REQ_0504` PDO representation) —
   and, per device, the ``<Slots>`` constraints as ``Option<Slots>``:
   slot increments, per-slot min/max instance counts, and the accepted
   ``ModuleIdent`` references with their ``Default`` markers. A file
   whose ``<Descriptions>`` carries modules but no devices shall parse.

   The parser shall **not** resolve a plugged module lineup into an
   effective PDO list or process image — slot expansion and increment
   arithmetic are consumer work (:need:`ADR_0102`), exactly as PDO
   assignment alternatives are captured unresolved per
   :need:`REQ_0504`.

   **Rationale.** Couplers such as the WAGO 750-354 describe their
   pluggable terminals via MDP; without the catalog and constraints a
   network configurator cannot model the device at all, but *which*
   modules are plugged is per-installation configuration the ESI cannot
   know. Implemented in ``crates/taktora-ethercat-esi/src/model.rs``
   and ``src/dto.rs``; verified by ``tests/modules.rs`` and the gated
   real-file test ``tests/wago_real.rs``.

.. req:: AlternativeSmMapping captured faithfully, never resolved
   :id: REQ_0529
   :status: implemented
   :satisfies: FEAT_0051
   :links: BB_0060, TEST_0871

   The parser shall capture each ``<AlternativeSmMapping>`` declared
   under ``<Info><VendorSpecific><TwinCAT>`` into a vendor-neutral,
   typed IR — its name, its default flag, and, per sync manager, the
   ordered list of assigned PDO indices with their optional
   ``ChannelNo`` — without resolving it to a process image, picking a
   default, or merging it with the per-PDO ``Sm=`` mapping. A device
   that declares no ``<AlternativeSmMapping>`` shall yield an empty
   set, not a fabricated default.

   **Rationale.** An ``<AlternativeSmMapping>`` enumerates one complete
   selectable PDO-assignment set spanning every sync manager (e.g. the
   EL7047 "Positioning interface" assigns SM2 = {0x1601, 0x1602,
   0x1606} and SM3 = {0x1a01, 0x1a03, 0x1a07}). Capturing each as a
   faithful set is what lets the codegen emit the joint per-device
   ``<Dev>OpMode`` enum (:need:`REQ_0523`); resolving it here would
   bake codegen policy into the parser, violating
   :need:`ADR_0102`. The capture-it-verbatim posture matches
   :need:`REQ_0504` (PDO alternatives kept unresolved) and
   :need:`REQ_0850` (MDP catalog kept unexpanded). issue #70.

.. req:: Vendor-specific extensions captured as opaque blobs
   :id: REQ_0505
   :status: implemented
   :satisfies: FEAT_0051
   :links: BB_0060, TEST_0403

   Vendor-specific ESI extensions (e.g. Beckhoff ``<Vendor:...>``
   elements) shall be retained in the IR as opaque ``RawXml`` blobs
   carrying the element name, attributes, and inner text/children.
   The parser shall not hard-fail on unknown vendor elements;
   downstream tools may inspect or ignore them.

.. req:: Parse errors carry line and column
   :id: REQ_0506
   :status: implemented
   :satisfies: FEAT_0051
   :links: BB_0060, TEST_0404

   ``EsiError`` variants raised during parsing shall carry the
   source line and column of the offending construct so build-time
   diagnostics point at the failing ESI file location.

   Syntax errors carry the source line and column (a ``Span``).
   Semantic value errors carry the offending element path and, where
   the parser can recover a position, a span; some semantic errors
   carry no position because the underlying ``quick-xml`` deserializer
   does not expose one. Implemented in
   ``crates/taktora-ethercat-esi/src/error.rs`` +
   ``src/position.rs``; verified by ``tests/errors.rs``.
