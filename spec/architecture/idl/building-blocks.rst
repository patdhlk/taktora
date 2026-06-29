Building block view
===================

arc42 §4 — the building blocks that realise the toolchain, one crate per
block. The dependency graph is :need:`ARCH_0081`; this page records each
crate's job, public surface, and the dependencies it is and is not
allowed to carry.

The diagram below reads the same crates as a public-API pipeline: each
layer's output type is the next layer's input, from ``.dbc`` bytes
through to the ``WireType`` surface the generated code implements.

.. mermaid::

   graph LR
       dbc["DBC text<br/>&str"]
       c["BB_0117 taktora-idl-core<br/>Module / Struct / Field IR<br/>validate() + max_serialized_len()"]
       w["BB_0118 taktora-idl-wire<br/>WireType + pack/unpack<br/>(no_std, no deps)"]
       d["BB_0119 taktora-idl-dbc<br/>parse() → DbcDatabase<br/>lower() → (Module, DbcLayout)"]
       g["BB_0114 taktora-idl-codegen<br/>resolve() + generate&lt;B&gt;<br/>naming + collision policy"]
       b["BB_0115 taktora-idl-codegen-can<br/>CanBackend: MessageBackend impl"]
       t["BB_0116 taktora-idl-codegen-can-tests<br/>build.rs round-trip harness"]

       dbc -->|"parse()"| d
       c -.->|"IR types"| d
       d -->|Module| g
       d -->|DbcLayout| b
       g -->|ResolvedModule| b
       b -->|TokenStream| t
       w -.->|generated code calls| t

.. building-block:: taktora-idl-core
   :id: BB_0117
   :status: implemented
   :implements: FEAT_0111

   The bounded message-type IR. Pure data carriers: ``Module``,
   ``Struct``, ``Field``, ``EnumDef``, ``EnumVariant``, ``Service``,
   ``Type`` (Scalar / String{capacity} / Array / Sequence{capacity} /
   Struct / Enum), and ``Scalar``. ``validate`` enforces structural
   soundness; ``max_serialized_len`` returns the finite bound. No wire
   format, no transport dep, no ``proc-macro2``/``quote``.

.. building-block:: taktora-idl-wire
   :id: BB_0118
   :status: implemented
   :implements: FEAT_0112

   The runtime crate. ``WireType`` (``MAX_SERIALIZED_LEN`` const, plus
   ``encode``/``decode`` over ``&[u8]``), the ``WireError`` closed error
   set, ``ByteOrder``, and the ``pack_unsigned`` / ``pack_signed`` /
   ``unpack_unsigned`` / ``unpack_signed`` bit primitives addressing both
   DBC bit-numbering conventions. ``#![no_std]``, allocation-free, zero
   external dependencies.

.. building-block:: taktora-idl-dbc
   :id: BB_0119
   :status: implemented
   :implements: FEAT_0113

   The DBC frontend. ``parse(text) -> Result<DbcDatabase, ParseError>``
   builds a faithful model (messages, signals, value tables,
   multiplexers); ``lower(&db, name) -> Result<LoweredDbc, LowerError>``
   emits an ``idl_core::Module`` plus a ``DbcLayout`` sidecar
   (``FrameLayout`` / ``SignalLayout``) keyed by source name. Depends on
   ``taktora-idl-core``; no codegen or transport dep.

.. building-block:: taktora-idl-codegen
   :id: BB_0114
   :status: implemented
   :implements: FEAT_0114

   The plane-generic codegen layer. Owns the ``naming`` policy
   (PascalCase / snake_case, keyword escaping, leading-digit prefixing)
   and its collision detection; defines the ``MessageBackend`` trait
   (``preamble`` / ``emit_enum`` / ``emit_struct``); exposes ``resolve``
   (→ ``ResolvedModule`` with idents chosen and fields classified by
   ``FieldKind``) and ``generate`` (→ ``proc_macro2::TokenStream``).
   Names no wire format.

.. building-block:: taktora-idl-codegen-can
   :id: BB_0115
   :status: implemented
   :implements: FEAT_0115

   The concrete CAN backend. ``CanBackend::new(&DbcLayout)`` implements
   ``MessageBackend``, emitting one ``WireType`` impl per message struct
   whose ``encode``/``decode`` bit-pack each field via the
   ``taktora-idl-wire`` primitives at the DBC-declared placement. Integer
   and enum fields this round; ``bool``/float rejected. Depends on
   ``taktora-idl-codegen`` and ``taktora-idl-dbc``.

.. building-block:: taktora-idl-codegen-can-tests
   :id: BB_0116
   :status: implemented
   :implements: FEAT_0115

   The build-time verification harness (``publish = false``). Its
   ``build.rs`` runs the full ``.dbc`` → IR → codegen → format pipeline
   over a fixture database and writes the generated module to
   ``OUT_DIR``; the crate compiles that module and round-trips it in
   tests. The slice's proof-of-life that generated ``WireType`` code
   actually compiles and whose ``encode``/``decode`` agree.
