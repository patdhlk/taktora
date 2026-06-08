Codegen IR and backend trait
============================

The codegen-side IR and the ``CodegenBackend`` trait (:need:`BB_0082`):
naming policy, collision handling, and PDO dedup, decoupled from both
the INI parser and any transport opinion.

.. feat:: Codegen IR and backend trait
   :id: FEAT_0063
   :status: open
   :satisfies: FEAT_0060

   The codegen-side IR (an extension of the parser IR with naming /
   collision policy applied) and the ``CodegenBackend`` trait that
   lets multiple emitters share that IR. This crate
   (``canopen-eds-codegen``) knows nothing about INI and nothing
   about taktora-connector-can.

.. req:: CodegenBackend trait shape
   :id: REQ_0730
   :status: open
   :satisfies: FEAT_0063

   The crate shall define a ``CodegenBackend`` trait with
   ``fn emit_device(&self, device: &eds::Device) -> Result<TokenStream,
   CodegenError>`` and ``fn emit_module_root(&self, devices: &[eds::Device])
   -> Result<TokenStream, CodegenError>``. The top-level entry point
   shall be ``fn generate<B: CodegenBackend>(eds: &EdsFile, backend:
   &B) -> Result<TokenStream, CodegenError>``.

.. req:: Naming policy is owned by codegen, not the backend
   :id: REQ_0731
   :status: open
   :satisfies: FEAT_0063

   The ``canopen-eds-codegen`` crate shall sanitise EDS
   ``ProductName`` strings into valid Rust identifiers (whitespace,
   hyphens, slashes, dots → ``_``; leading digit prefixed with
   ``_``) before invoking the backend. Backends shall receive
   idents pre-validated; they shall not re-implement sanitisation.

.. req:: Revision collision handled deterministically
   :id: REQ_0732
   :status: open
   :satisfies: FEAT_0063

   When two EDS files share ``ProductName`` but differ in
   ``RevisionNumber`` (OD index ``0x1018:03``), the codegen layer
   shall disambiguate using a deterministic ``_REV<hex>`` suffix
   derived from the raw ``RevisionNumber``. Input file order shall
   not affect the generated idents.

.. req:: Common PDO entry types deduplicated
   :id: REQ_0733
   :status: open
   :satisfies: FEAT_0063

   When two or more devices' PDOs include structurally identical
   entry layouts (same bit-len + data-type tuple list), the codegen
   layer shall emit one shared PDO entry struct referenced by both
   devices rather than two duplicated structs. Structural equality
   is the dedup key; field names do not need to match.

.. req:: Emission target is proc_macro2 TokenStream
   :id: REQ_0734
   :status: open
   :satisfies: FEAT_0063

   The codegen layer shall produce ``proc_macro2::TokenStream``
   values and assemble them with ``quote!``. String-templated
   emission (``format!`` + write) is rejected — token-level
   construction preserves span / hygiene and yields rustfmt-able
   output via ``prettyplease`` (per :need:`ADR_0076`).

.. req:: One EDS file equals one device
   :id: REQ_0735
   :status: open
   :satisfies: FEAT_0063

   The codegen layer shall treat one EDS file as one device.
   ``emit_module_root`` shall accept ``&[EdsFile]`` and emit one
   device per file plus the shared registry table per
   :need:`REQ_0745`.
