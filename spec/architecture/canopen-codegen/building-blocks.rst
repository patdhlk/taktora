Building block view
===================

arc42 §4 — the building blocks that realise the toolchain, one crate
per block. The dependency graph is :need:`ARCH_0070`; this page records
each crate's job, public surface, and the dependencies it is and is not
allowed to carry.

The diagram below reads the same crates as a public-API pipeline: each
layer's output type is the next layer's input, from EDS bytes through
to the runtime trait surface the generated drivers implement.

.. mermaid::

   graph LR
       ini["EDS INI<br/>&str"]
       odc["BB_0080 fieldbus-od-core<br/>Identity, DictEntry,<br/>PdoEntry, PdoMap"]
       p["BB_0081 canopen-eds<br/>parse() → EdsFile IR"]
       g["BB_0082 canopen-eds-codegen<br/>generate&lt;B&gt;(IR, &amp;B) → TokenStream<br/>naming + dedup + collision policy"]
       b["BB_0083 -codegen-taktora<br/>CodegenBackend impl<br/>(sole canopen-eds-rt dep)"]
       rt["BB_0084 canopen-eds-rt<br/>CanOpenDevice / CanOpenConfigurable<br/>(no transport dep)"]
       build["BB_0085 canopen-eds-build<br/>build.rs → prettyplease → $OUT_DIR"]
       cli["BB_0086 canopen-eds-cli<br/>cargo eds expand / list"]
       ver["BB_0087 canopen-eds-verify<br/>verify(eds, dump) → VerifyReport"]
       adapter["BB_0088 connector CanOpenDevice adapter (follow-on)"]

       ini --> odc
       odc --> p
       p -->|EdsFile| g
       g -->|CodegenBackend| b
       b -->|implements| rt
       g -->|TokenStream| build
       g -->|TokenStream| cli
       p -->|EdsFile| ver
       rt --> adapter

.. building-block:: fieldbus-od-core
   :id: BB_0080
   :status: open
   :implements: FEAT_0061

   The shared OD types lifted out of ``ethercat-esi``. Pure data
   carriers: ``Identity``, ``DataType`` (CiA 301 enumeration),
   ``AccessRights``, ``DictEntry``, ``PdoEntry``, ``PdoMap``. No
   I/O, no serde in the default surface, no transport dep.

.. building-block:: canopen-eds parser crate
   :id: BB_0081
   :status: open
   :implements: FEAT_0062

   INI parser front-end. ``parse(text) -> Result<EdsFile, EdsError>``.
   Uses a serde-derive backend (``serde_ini`` primary candidate).
   Liberal-quirk policy raises warnings rather than failing
   (:need:`REQ_0725`). Unknown sections retained as ``RawSection``.

.. building-block:: canopen-eds-codegen
   :id: BB_0082
   :status: open
   :implements: FEAT_0063

   Codegen IR + ``CodegenBackend`` trait. Owns naming policy
   (sanitisation, revision-suffix), structural dedup of PDO entry
   shapes, and the ``generate<B: CodegenBackend>`` entry point.

.. building-block:: canopen-eds-codegen-taktora
   :id: BB_0083
   :status: open
   :implements: FEAT_0064

   The opinionated concrete backend. Emits one device struct per
   EDS, sum-typed RPDO / TPDO enums (one variant per declared
   mapping), an ``Identity`` const per device, a module-root
   registry mapping ``Identity → factory``, and ``impl
   CanOpenConfigurable`` bodies driving bring-up SDO writes.

.. building-block:: canopen-eds-rt
   :id: BB_0084
   :status: open
   :implements: FEAT_0065

   The runtime trait crate. ``CanOpenDevice`` (sync RPDO/TPDO
   methods, identity const, NMT state getter/setter) and
   ``CanOpenConfigurable`` (async ``configure`` over a caller-
   supplied ``SdoClient``).

.. building-block:: canopen-eds-build
   :id: BB_0085
   :status: open
   :implements: FEAT_0066

   ``build.rs`` glue. Drives glob → parse → codegen → backend →
   ``prettyplease::unparse`` → write to ``$OUT_DIR``. Emits one
   ``cargo:rerun-if-changed`` per matched EDS and one for the
   build script. Surfaces parser warnings as ``cargo:warning=``
   lines.

.. building-block:: canopen-eds-cli
   :id: BB_0086
   :status: open
   :implements: FEAT_0067

   ``cargo eds expand`` / ``cargo eds list`` subcommand. Shares
   ``canopen-eds`` + ``canopen-eds-codegen-taktora`` as libraries;
   output is byte-identical to the build helper's per-device slice.

.. building-block:: canopen-eds-verify
   :id: BB_0087
   :status: open
   :implements: FEAT_0068

   Offline EDS vs JSON SDO-dump diff. Consumes the same ``EdsFile``
   IR as the codegen path; JSON dump decoding lives inside the
   verifier. ``0 / 1 / 2`` exit code on match / mismatch / error.

.. building-block:: taktora-connector-can adapter (follow-on)
   :id: BB_0088
   :status: open
   :implements: FEAT_0060
   :links: FEAT_0046

   Out-of-scope for this round. A thin adapter that maps any
   ``CanOpenDevice`` into the connector's frame plumbing. Lookup
   from ``Identity`` to factory via the generated registry
   (:need:`REQ_0745`); resolution from inbound CAN ID to RPDO
   enumeration via the configured ``0x1400..0x14FF`` comm
   parameters. Tracked here so the umbrella diagram (:need:`ARCH_0070`)
   is complete; deliverable belongs to a follow-on spec
   (:need:`REQ_0795`).
