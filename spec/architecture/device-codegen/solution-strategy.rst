Solution strategy
=================

arc42 §4 — the toolchain's shape is the consequence of eight
architectural decisions. Each is captured as an ADR that ``:refines:``
the requirement or feature it answers.

.. arch-decision:: Parser separated from codegen (strict layering)
   :id: ADR_0070
   :status: open
   :refines: FEAT_0050

   **Context.** A monolithic "ESI to ethercrab driver" crate would
   conflate three concerns — XML parsing, IR shape and naming
   policy, and ethercrab-specific code emission. Downstream tools
   that need only one of these (a TwinCAT importer, a network
   configurator, the EEPROM verifier) would be forced to compile
   ethercrab and ``quote!`` machinery.

   **Decision.** Three crates: ``ethercat-esi`` (parser, no_std),
   ``ethercat-esi-codegen`` (IR + backend trait, no ethercrab),
   ``ethercat-esi-codegen-ethercrab`` (concrete backend). The
   parser owns no_std purity; the codegen owns naming and
   collision policy; the backend owns ethercrab opinion.

   **Consequences.** ✅ Each layer has one job and one
   reason-to-change. ✅ Future backends (soem, minimal, no_std-only)
   are net-additive — no parser or codegen churn.
   ❌ Three crates instead of one — more ``Cargo.toml`` surface
   for the eventual maintainer. Mitigated by the workspace being
   single-repo.

.. arch-decision:: Two-trait runtime split (EsiDevice + EsiConfigurable)
   :id: ADR_0071
   :status: open
   :refines: FEAT_0054

   **Context.** The hot path (cyclic ``decode_inputs`` /
   ``encode_outputs``) is synchronous and called from the
   cycle-loop. The bring-up path (``configure``) is async, issues
   SDO writes through the ethercrab API, and only runs during
   preop. Mixing them under one trait would force either: (a) an
   ``async fn`` on the hot-path methods (forbidden, wrong
   semantics), or (b) a sync ``configure`` that can't talk to
   ethercrab's async SDO writes.

   **Decision.** Two traits. ``EsiDevice`` is sync, mandatory,
   owns the hot path. ``EsiConfigurable: EsiDevice`` is async,
   optional (a device without configurable PDO mappings can skip
   it), owns the preop SDO sequence.

   **Consequences.** ✅ Hot path stays sync and zero-cost.
   ✅ Devices that don't need preop SDO writes don't need to
   implement the async trait. ❌ Two trait names instead of one;
   the consumer-side dispatcher has to handle both shapes.
   Acceptable cost; the generated code shoulders most of the
   burden.

.. arch-decision:: PDO assignment alternatives as sum types
   :id: ADR_0072
   :status: open
   :refines: FEAT_0053

   **Context.** An ESI device commonly declares 2–4 PDO
   assignment alternatives (e.g. "Standard 16-bit", "Compact
   8-bit"). One representation in Rust is a single struct with
   ``Option<…>`` fields for each alternative's PDO entries —
   convenient but lossy: invalid combinations (two alternatives
   enabled at once) are representable.

   **Decision.** Emit an enum ``<Device>PdoAssignment`` with one
   variant per alternative, plus one ``<Device>Pdo<Variant>``
   struct per variant. The device struct's ``pdo`` field is the
   enum; selecting a variant chooses both the assignment
   bitfields and the typed PDO struct.

   **Consequences.** ✅ "Two alternatives at once" is
   unrepresentable. ✅ Per-variant PDO structs have ``Default``
   and ``Clone``; switching alternatives at runtime is a
   ``self.pdo = …`` assignment. ❌ Slightly more generated code
   per device (one enum + N structs instead of one struct).
   Negligible at the device counts in scope (<100).

.. arch-decision:: Joint per-device OpMode enum supersedes per-direction PDO-assignment alternatives
   :id: ADR_0104
   :status: accepted
   :refines: ADR_0072
   :links: REQ_0523

   **Context.** :need:`ADR_0072` modelled PDO-assignment choice
   per-direction, on the assumption that an ESI device offers a small
   menu of interchangeable RxPDO / TxPDO alternatives. Two facts about
   real Beckhoff ESI break that model. First, Beckhoff marks selectable
   PDOs ``Fixed="1"`` — which the original codegen read as "always
   assigned", when ``Fixed`` actually means only that the PDO's *entry
   list is not editable* (orthogonal to whether the PDO is assigned to a
   sync manager). Second, a valid sync-manager assignment is a **set
   spanning both directions at once**, not an independent per-direction
   pick: the EL7047 "Positioning interface" mode assigns
   SM2 = {0x1601, 0x1602, 0x1606} **and** SM3 = {0x1a01, 0x1a03,
   0x1a07} as one indivisible choice. A per-direction sum type can
   represent neither a multi-PDO assignment within one direction nor the
   SM2↔SM3 pairing that makes the choice coherent.

   **Decision.** Resolve each device to a *set of selectable
   assignments* (one per ``<AlternativeSmMapping>``, else a single
   default) and emit one **joint** ``<Dev>OpMode`` enum — one variant
   per assignment, each variant carrying the complete
   ``{ inputs, outputs }`` for that mode (:need:`REQ_0523`,
   :need:`REQ_0524`). The per-direction ``classify_alternatives`` /
   ``MultipleAlternativeGroups`` machinery and the
   ``<IDENT>PdoAssignment`` sum type are retired. This **refines**
   :need:`ADR_0072`'s "make illegal states unrepresentable" principle,
   moving its granularity from per-direction to per-device: the illegal
   state now ruled out is "two operating modes active at once", and the
   pairing between an RxPDO set and its TxPDO set is captured by
   construction.

   **Consequences.** ✅ ``Fixed`` is decoupled from "always-on": the
   default assignment derives from ``Sm=`` / ``Mandatory``, not
   ``Fixed`` (:need:`REQ_0527`). ✅ Every device is uniformly
   ``struct <Dev> { mode: <Dev>OpMode }``; a single-mode device simply
   gets a one-variant ``Default`` enum, so there is no special case for
   "non-selectable" devices. ✅ Multi-PDO and cross-direction
   assignments are now expressible. ❌ This is pre-v1.0 churn: the
   golden snapshot output changes for **every** device, since even
   single-mode devices move from a bare struct to the
   ``struct { mode }`` shape. Acceptable before the first release.

.. arch-decision:: Future CANopen support via shared OD IR
   :id: ADR_0073
   :status: accepted
   :refines: FEAT_0050
   :links: ADR_0078

   **Context.** EtherCAT's CoE inherits the CANopen Object
   Dictionary (CiA 301) wholesale: index/subindex addressing,
   PDO mapping objects, the data-type table. EDS (CANopen) and
   ESI (EtherCAT) describe overlapping object dictionaries.

   **Decision.** When CANopen support is in scope, the OD IR is
   extracted to a shared ``fieldbus-od-core`` crate. ``ethercat-esi``
   shrinks to "ESI-specific elements + reference to OD core".
   ``canopen-eds`` (new) parses CiA-306 INI and emits the same OD
   IR. The runtime traits are **not** merged (per :need:`REQ_0592`)
   — CANopen's event-driven transport gets its own ``CanOpenDevice``
   family in a separate crate.

   **Consequences.** ✅ ~70 % of code is shared at the IR level
   when CANopen lands. ✅ Each transport keeps its honest runtime
   shape. ❌ Future refactor required to lift OD types out of
   ``ethercat-esi``. Manageable — the lift is mechanical because
   parser and IR are already decoupled (per :need:`ADR_0070`).

   **Closure.** The OD-core lift (:need:`ADR_0078`) is realised by the
   ``taktora-fieldbus-od-core`` crate (2026-05), which now holds the
   shared ``Identity``, ``DataType``, and ``DictEntry`` types;
   ``taktora-ethercat-esi`` depends on it and re-exports them, and
   ``taktora-ethercat-netcfg`` consumes the shared ``Identity``.
   First-class ``FEAT``/``REQ`` authoring for
   ``taktora-fieldbus-od-core`` as a standalone feature is a follow-on
   round, paired with the CANopen consumer (:need:`FEAT_0060`) that
   motivates a *shared* OD crate.

.. arch-decision:: Vendor extensions captured as opaque blobs
   :id: ADR_0074
   :status: open
   :refines: FEAT_0051

   **Context.** Vendor-specific ``<Vendor:Foo>`` elements appear
   in real-world ESI files (notably Beckhoff). Three policies are
   possible: hard-fail on unknown elements (rejects most real
   files), warn (reports but parses), capture as opaque blobs
   (parses, retains the data, defers interpretation).

   **Decision.** Capture as opaque blobs. The IR carries a
   ``RawXml { name, attributes, inner }`` for every unrecognised
   element. The codegen layer ignores them; downstream tools
   (e.g. a Beckhoff-specific importer) can interpret them.

   **Consequences.** ✅ Real-world ESI files parse without
   bespoke patches per vendor. ✅ Information is preserved, not
   discarded. ❌ The IR carries a payload nobody on the
   ethercrab-backend side reads. Negligible cost; the alternative
   (parse-and-discard) is worse.

.. arch-decision:: Object dictionary as static table, feature-gated
   :id: ADR_0075
   :status: open
   :refines: FEAT_0054

   **Context.** OD-heavy devices (e.g. Beckhoff ELxxx coupling
   modules with 200+ entries) can balloon generated code by
   10–50× if every OD entry becomes a match arm. Three options:
   match arms (large, fast lookup), static table (smaller, linear
   lookup but O(log n) with binary search on sorted index),
   no emission (smallest, fails the "OD-aware diagnostics" use
   case).

   **Decision.** Emit OD entries as a sorted
   ``static OD: &[(u16, u8, DataType, &str)]`` table, gated
   behind a default-off ``object-dictionary`` cargo feature on
   the generated module's parent crate.

   **Consequences.** ✅ Default builds carry zero OD-related
   code or rodata. ✅ Diagnostic tools that need OD lookup
   ``cargo build --features object-dictionary`` and binary-search
   the sorted slice. ❌ OD-aware code is a separate compile
   profile; CI must cover both.

.. arch-decision:: Use prettyplease, not rustfmt, for emit formatting
   :id: ADR_0076
   :status: open
   :refines: FEAT_0055

   **Context.** Emitting raw ``TokenStream`` produces single-line
   files that are unreadable when inspected. Two formatting
   options: shell out to ``rustfmt`` (requires a working rustfmt
   binary at build time; cargo doesn't guarantee one) or call
   ``prettyplease::unparse`` (a library; works offline; smaller
   formatter than rustfmt).

   **Decision.** ``prettyplease`` in-process. No shell-out to
   rustfmt.

   **Consequences.** ✅ Build helper has no external command
   dependency. ✅ Output is stable across rustfmt versions.
   ❌ Slightly different formatting than the rest of the
   workspace's rustfmt-formatted code (prettyplease is opinionated
   but not 100 % rustfmt-compatible). Acceptable — generated
   files are not human-edited.

.. arch-decision:: cargo subcommand for inspection, not proc-macro
   :id: ADR_0077
   :status: open
   :refines: FEAT_0056

   **Context.** Two inspection mechanisms were on the table.
   Option A: an ``esi_device!("EL3001.xml")`` proc-macro that
   inserts the device's generated code at call sites — gives the
   IDE rust-analyzer hover info at the call site, doubles the
   codegen path. Option B: a ``cargo esi expand`` subcommand that
   prints generated code to stdout — no IDE integration, one
   codegen path.

   **Decision.** Cargo subcommand only. Proc-macro form is
   rejected per :need:`REQ_0591`.

   **Consequences.** ✅ One codegen path; tests run once. ✅ No
   proc-macro compile-time cost on every workspace ``cargo
   build``. ❌ IDE doesn't surface generated symbols on hover.
   Acceptable; the generated file is a regular file in
   ``$OUT_DIR`` that rust-analyzer indexes.

.. arch-decision:: std/POSIX baseline for the parser and OD-core crates
   :id: ADR_0097
   :status: accepted
   :refines: FEAT_0051

   **Context.** :need:`REQ_0501` and :need:`CON_0013` specified a
   ``no_std`` + ``alloc`` baseline. In practice taktora targets a POSIX
   OS today, and ``no_std`` forced a hand-rolled error type (no
   ``core::error::Error``) and blocked ``thiserror``-derived errors
   carrying source positions (:need:`REQ_0506`).

   **Decision.** ``taktora-ethercat-esi`` and the new
   ``taktora-fieldbus-od-core`` crate target ``std``. ``thiserror`` v2
   provides the error derive. ``no_std`` is deferred, not abandoned;
   genuinely ``no_std`` crates (e.g. ``taktora-bounded-alloc``) are
   unaffected.

   **Consequences.** :need:`REQ_0501` is rejected and :need:`CON_0013`'s
   scope narrows to the runtime-trait crate (:need:`FEAT_0054`),
   revisited in its own round. The parser gains located,
   ``core::error::Error``-implementing errors.

.. arch-decision:: Object-safe EsiDevice, identity reuse, ethercrab behind SdoWrite
   :id: ADR_0098
   :status: accepted
   :refines: FEAT_0054
   :links: ADR_0078, ADR_0097

   **Context.** The runtime-trait surface as first specified carried
   three latent contradictions. (1) :need:`REQ_0530` put
   ``const IDENTITY`` on the ``EsiDevice`` trait, but an associated
   const makes a trait ``dyn``-incompatible — directly breaking the
   ``Box<dyn EsiDevice>`` registry factories :need:`REQ_0525`
   mandates. (2) :need:`BB_0063` had ``ethercat-esi-rt`` depend on
   ``ethercrab`` *and* be ``no_std``, which is impossible (ethercrab
   is a ``std`` async-networking crate) and would force every
   ``EsiDevice`` consumer — a simulator, a TwinCAT importer — to
   compile ethercrab's full stack, defeating :need:`REQ_0532` /
   :need:`QG_0013`. (3) The spec named a fresh ``SubDeviceIdentity``
   type alongside the already-adopted od-core ``Identity``
   (:need:`ADR_0078`) and ethercrab's own ``SubDeviceIdentity``,
   re-opening the type-triplication the OD-core lift had just closed.

   **Decision.** Resolve all three at the trait crate:

   * ``EsiDevice`` is **object-safe** — identity is the method
     ``fn identity(&self) -> Identity``, not an associated const. The
     static identity value lives in the standalone ``const`` of
     :need:`REQ_0522`; the registry keys on that const and stores
     ``Box<dyn EsiDevice>`` factories.
   * ``EsiConfigurable::configure`` is **generic over a minimal**
     ``SdoWrite`` **trait** (:need:`REQ_0535`) instead of naming
     ``SubDevicePreOperational``. ``ethercat-esi-rt`` thus has no
     ethercrab dependency; the concrete
     ``impl SdoWrite for SubDevicePreOperational`` lives in the
     backend (:need:`REQ_0520`).
   * Identity is the shared od-core ``Identity``
     (vendor_id / product_code / revision); no ``SubDeviceIdentity``
     is minted. Mapping onto ethercrab's wire identity (which adds
     ``serial``) is the connector adapter's job (:need:`BB_0067`).

   **Consequences.** ✅ The ``Box<dyn EsiDevice>`` registry compiles.
   ✅ ``EsiDevice`` consumers pay nothing for ethercrab. ✅ Generated
   ``configure`` is unit-testable against a mock ``SdoWrite``.
   ✅ One identity type across the toolchain. ❌ Deviates from the
   literal wording of :need:`REQ_0530` / :need:`REQ_0531` /
   :need:`REQ_0522` / :need:`REQ_0512`, now reworded to match. ❌ A
   thin extra trait (``SdoWrite``) and one generic on ``configure``.

.. arch-decision:: The ESI parser emits a faithful IR and never resolves configuration
   :id: ADR_0102
   :status: accepted
   :refines: FEAT_0051
   :links: REQ_0504, REQ_0849, REQ_0850

   **Context.** Several ESI constructs describe a *space* of possible
   device configurations rather than one concrete configuration: PDO
   assignment alternatives (multiple PDOs competing for a sync
   manager), MDP module slots (pluggable terminals chosen per
   installation), and the SII source data (whose meaning depends on
   verifier policy). Each parser extension re-raises the same question:
   should the parser resolve these into something concrete — a flat
   process image, an effective PDO list, decoded PDI fields?

   **Decision.** No. The parser captures what the document declares,
   structurally and faithfully, and stops there: PDO alternatives are
   kept per declared element with no bit offsets (:need:`REQ_0504`),
   the MDP catalog and slot constraints are kept unexpanded
   (:need:`REQ_0850`), and EEPROM payloads are decoded to bytes but not
   interpreted (:need:`REQ_0849`). Resolution — picking alternatives,
   expanding a plugged lineup, assembling or checking an SII image —
   is consumer work (codegen, network configurator, verifier), where
   the missing inputs (the user's terminal lineup, the verifier's
   policy) actually live.

   **Consequences.** ✅ One parse result serves every consumer
   regardless of its policy. ✅ Parser tests need no configuration
   fixtures, only documents. ✅ Consumer resolution logic is testable
   against a stable IR. ❌ Every consumer that needs a process image
   must implement (or share) resolution logic — the netcfg modular
   round will design that layer against this boundary. Lossless
   re-representations (hex → bytes, ``#x`` → integers) are NOT
   resolution and stay in the parser.
