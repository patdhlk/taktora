Building block view
===================

arc42 §5 — the building blocks that realise the toolchain, one crate
per block. The dependency graph is :need:`ARCH_0050`; this page records
each crate's job, public surface, and the dependencies it is and is not
allowed to carry.

The diagram below reads the same crates as a public-API pipeline: each
layer's output type is the next layer's input, from ESI bytes through
to the runtime trait surface the generated drivers implement.

.. mermaid::

   graph LR
       xml["ESI XML<br/>&str"]
       p["BB_0060 ethercat-esi<br/>parse() → EsiFile IR"]
       g["BB_0061 ethercat-esi-codegen<br/>generate&lt;B&gt;(IR, &amp;B) → TokenStream<br/>naming + dedup + collision policy"]
       b["BB_0062 -codegen-ethercrab<br/>CodegenBackend impl<br/>(sole ethercrab dep)"]
       rt["BB_0063 ethercat-esi-rt<br/>EsiDevice / EsiConfigurable<br/>+ SdoWrite (no ethercrab)"]
       build["BB_0064 ethercat-esi-build<br/>build.rs → prettyplease → $OUT_DIR"]
       cli["BB_0065 ethercat-esi-cli<br/>cargo esi expand / list"]
       ver["BB_0066 ethercat-esi-verify<br/>verify(xml, sii) → VerifyReport"]
       adapter["BB_0067 connector EsiDevice adapter"]

       xml --> p
       p -->|EsiFile| g
       g -->|CodegenBackend| b
       b -->|implements| rt
       g -->|TokenStream| build
       g -->|TokenStream| cli
       p -->|EsiFile| ver
       rt --> adapter

.. building-block:: ethercat-esi (parser crate)
   :id: BB_0060
   :status: open
   :implements: FEAT_0051
   :refines: REQ_0843

   The parse crate. Reads ESI XML via ``quick-xml`` +
   ``serde``, emits ``EsiFile`` IR. ``std`` baseline per
   :need:`ADR_0097`. Public API is
   ``pub fn parse(xml: &str) -> Result<EsiFile, EsiError>``
   and the ``EsiFile`` / ``Device`` / ``Pdo`` / ``DictEntry``
   types per :need:`REQ_0504`. Carries no dependency on
   ``ethercrab``, ``proc-macro2``, or any codegen crate.

.. building-block:: ethercat-esi-codegen (IR + backend trait)
   :id: BB_0061
   :status: open
   :implements: FEAT_0052

   Codegen layer. Owns the ``CodegenBackend`` trait
   (:need:`REQ_0510`), naming sanitisation (:need:`REQ_0511`),
   revision-disambiguation (:need:`REQ_0512`), and PDO entry
   deduplication (:need:`REQ_0513`). Depends on ``ethercat-esi``
   (left) and ``proc-macro2`` / ``quote`` / ``prettyplease``
   (right). Does not depend on ``ethercrab``.

.. building-block:: ethercat-esi-codegen-ethercrab (concrete backend)
   :id: BB_0062
   :status: open
   :implements: FEAT_0053

   The one concrete backend shipped in this round. Emits per-device
   structs implementing ``EsiDevice`` and (where the device has
   configurable PDO mappings) ``EsiConfigurable``. Sole crate in
   the toolchain that depends on ``ethercrab`` (:need:`REQ_0520`).

.. building-block:: ethercat-esi-rt (runtime trait crate)
   :id: BB_0063
   :status: open
   :implements: FEAT_0054

   The minimal trait crate consumed by generated devices and
   adapters. Owns the object-safe ``EsiDevice``, ``EsiConfigurable``,
   the ``SdoWrite`` abstraction (:need:`REQ_0535`), and a *runtime*
   ``EsiError``; re-exports ``Identity`` from
   ``taktora-fieldbus-od-core`` rather than minting a
   ``SubDeviceIdentity``. Depends on ``bitvec`` only — **no
   ethercrab dependency**: the ethercrab ``SubDevicePreOperational``
   is reached through the ``SdoWrite`` trait, whose concrete impl
   lives in the backend (:need:`BB_0062`). ``std`` baseline today,
   ``no_std`` revisited per :need:`ADR_0097` reasoning. Deliberately
   thin so the contract is small. See :need:`ADR_0098`.

.. building-block:: ethercat-esi-build (build.rs glue)
   :id: BB_0064
   :status: open
   :implements: FEAT_0055

   Build-script helper consumed by downstream crates from their
   ``build.rs``. One method: ``Builder::new().glob(...).backend(...)
   .out_file(...).build()``. Wires parse → codegen → prettyplease
   → write to ``$OUT_DIR``. Emits ``cargo:rerun-if-changed`` per
   :need:`REQ_0542`.

.. building-block:: ethercat-esi-cli (cargo subcommand)
   :id: BB_0065
   :status: open
   :implements: FEAT_0056

   Cargo subcommand binary providing ``cargo esi expand`` and
   ``cargo esi list``. Pulls in ``ethercat-esi`` and
   ``ethercat-esi-codegen-ethercrab`` as library deps, formats
   output with ``prettyplease`` (re-using :need:`REQ_0543`).
   Binary lives outside any build script — invoked by the user,
   not by cargo on every build.

.. building-block:: ethercat-esi-verify (EEPROM diff tool)
   :id: BB_0066
   :status: open
   :implements: FEAT_0057

   Cross-validates ESI XML against captured SII EEPROM ``.bin``
   dumps. Standalone binary plus library API
   (``fn verify(xml: &str, sii: &[u8]) -> Result<VerifyReport,
   VerifyError>``). Depends on ``ethercat-esi`` only; the SII
   decoder lives in this crate to keep :need:`REQ_0520` honest.

.. building-block:: taktora-connector-ethercat EsiDevice adapter
   :id: BB_0067
   :status: open
   :implements: FEAT_0050

   The thin glue inside ``taktora-connector-ethercat`` (:need:`BB_0030`
   neighbourhood) that maps any ``EsiDevice`` into whatever
   internal device-trait the connector consumes. Written once, not
   touched per terminal addition. Concrete shape is local to the
   connector crate and out of scope for this spec; this BB exists
   to record the adapter as the *only* place where the codegen
   toolchain touches the runtime connector.
