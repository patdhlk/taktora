Plane-generic codegen
=====================

The plane-generic codegen layer (:need:`BB_0114`): the naming policy, the
``MessageBackend`` trait, and the ``resolve`` + ``generate`` entry point.
The message-plane twin of the device-plane ``esi-codegen`` — it knows
nothing about CAN, CDR, or any wire format; a backend owns that.

.. feat:: Plane-generic codegen
   :id: FEAT_0114
   :status: implemented
   :satisfies: FEAT_0110

   A new crate ``taktora-idl-codegen`` that resolves a validated
   ``idl_core::Module`` into a backing-with-identifiers-chosen form and
   hands it to a ``MessageBackend`` that emits Rust tokens. The crate
   owns naming and collision policy; it owns no wire format.

.. req:: Deterministic naming policy with collision detection
   :id: REQ_0954
   :status: implemented
   :satisfies: FEAT_0114
   :links: BB_0114, TEST_0930

   ``taktora-idl-codegen`` shall own the policy that maps verbatim source
   names to Rust identifiers: PascalCase for types and enum variants,
   snake_case for fields, escaping of Rust keywords, and prefixing of
   leading-digit names. When the policy maps two distinct source names
   onto one identifier (type, field, or variant), ``resolve`` shall fail
   with a collision error rather than emit colliding code.

.. req:: MessageBackend trait and resolve/generate entry point
   :id: REQ_0955
   :status: implemented
   :satisfies: FEAT_0114
   :links: BB_0114, TEST_0930, TEST_0931

   ``taktora-idl-codegen`` shall expose ``resolve(&Module) ->
   Result<ResolvedModule, CodegenError>`` (identifiers chosen, every
   field classified by its on-wire kind), a ``MessageBackend`` trait
   (``preamble`` / ``emit_enum`` / ``emit_struct``), and ``generate(&Module,
   &dyn MessageBackend) -> Result<TokenStream, CodegenError>`` returning
   a ``proc_macro2::TokenStream``. The returned stream shall be
   unformatted — a build layer renders it (``prettyplease``). The crate
   shall name no wire format.
