IR and codegen backend trait
============================

The codegen-side IR and the ``CodegenBackend`` trait (:need:`BB_0061`)
that lets multiple emitters share it — XML-free, ethercrab-free.

.. feat:: IR and codegen backend trait
   :id: FEAT_0052
   :status: open
   :satisfies: FEAT_0050

   The codegen-side IR (an extension of the parser IR with naming /
   collision policy applied) and the ``CodegenBackend`` trait that
   lets multiple emitters share that IR. This crate
   (``ethercat-esi-codegen``) knows nothing about XML and nothing
   about ethercrab.

.. req:: CodegenBackend trait shape
   :id: REQ_0510
   :status: open
   :satisfies: FEAT_0052

   The crate shall define a ``CodegenBackend`` trait with
   ``fn emit_device(&self, device: &esi::Device) -> Result<TokenStream,
   CodegenError>`` and ``fn emit_module_root(&self, devices: &[esi::Device])
   -> Result<TokenStream, CodegenError>``. The top-level entry point
   shall be ``fn generate<B: CodegenBackend>(esi: &EsiFile, backend:
   &B) -> Result<TokenStream, CodegenError>``.

.. req:: Naming policy is owned by codegen, not the backend
   :id: REQ_0511
   :status: implemented
   :satisfies: FEAT_0052
   :links: BB_0061, TEST_0410

   The ``ethercat-esi-codegen`` crate shall sanitise ESI product
   names into valid Rust identifiers (e.g. ``EL3001-0000`` →
   ``EL3001_0000``) before invoking the backend. Backends shall
   receive idents pre-validated; they shall not be expected to
   re-implement sanitisation.

.. req:: Revision collision handled deterministically
   :id: REQ_0512
   :status: implemented
   :satisfies: FEAT_0052
   :links: BB_0061, TEST_0411

   When two devices in the input set share a product name but differ
   in revision (e.g. ``EL3204`` rev ``0x00100000`` vs rev
   ``0x00110000``), the codegen layer shall disambiguate them using
   a deterministic suffix derived from the full 32-bit revision
   rendered as eight hex digits (``REV{revision:08X}``), producing
   distinct Rust idents (``EL3204_REV00100000`` vs
   ``EL3204_REV00110000``). The full-width form is collision-proof
   (a 4-digit high-word form would alias revisions differing only in
   the low word). The per-device identity ``const``
   (:need:`REQ_0522`) always carries this suffix; the struct name
   carries it only when a product-name collision requires
   disambiguation. Ordering of input files shall not affect the
   generated idents.

.. req:: Common PDO entry types deduplicated
   :id: REQ_0513
   :status: open
   :satisfies: FEAT_0052

   When two or more devices' PDOs include structurally identical
   entry layouts (same field order, same bit lengths, same data
   types), the codegen layer shall emit one shared PDO entry struct
   referenced by both devices rather than two duplicated structs.
   Structural equality is the deduplication key — names do not need
   to match.

.. req:: Emission target is proc_macro2 TokenStream
   :id: REQ_0514
   :status: open
   :satisfies: FEAT_0052

   The codegen layer shall produce ``proc_macro2::TokenStream``
   values and assemble them with ``quote!``. String-templated
   emission (``format!`` + write) is rejected — token-level
   construction preserves span / hygiene and yields rustfmt-able
   output via ``prettyplease``.
