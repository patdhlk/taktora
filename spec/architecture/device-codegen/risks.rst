Risks and technical debt
========================

arc42 §11.

.. risk:: OD table size blow-up on coupling modules
   :id: RISK_0010
   :status: open
   :links: ADR_0075, REQ_0533

   Beckhoff coupling modules can declare 200+ OD entries. With
   ``object-dictionary`` enabled, the static OD table per coupler
   reaches ~10 KB of rodata. Mitigated by the feature flag
   (:need:`ADR_0075`); becomes a tracked debt if a downstream
   consumer enables the feature and ships to constrained MCU
   targets. Not yet a problem in the current taktora deployment
   (Linux gateway only — :need:`REQ_0325`).

.. risk:: Beckhoff vendor extensions churn the IR
   :id: RISK_0011
   :status: open
   :links: ADR_0074, REQ_0505

   Beckhoff ships ESI files with ``<Vendor:Foo>`` elements that
   evolve between TwinCAT releases. Opaque-blob capture
   (:need:`ADR_0074`) keeps the parser stable, but downstream
   importers that interpret vendor blobs will need version
   awareness. Mitigation: keep vendor-blob interpretation in
   per-vendor importer crates, not in the parser or backend.

.. risk:: ethercrab API churn breaking the backend
   :id: RISK_0012
   :status: open
   :links: CON_0011, BB_0062

   ``ethercrab`` is pre-1.0 and its API has evolved
   (SubDevice / MainDevice rename, async signature changes). A
   minor-version bump can require a backend re-emit. Mitigation:
   pin ethercrab in ``ethercat-esi-codegen-ethercrab``'s
   ``Cargo.toml`` to the same range as
   ``taktora-connector-ethercat``; bump in lockstep.

.. risk:: ESI XML schema drift across vendors
   :id: RISK_0013
   :status: open
   :links: CON_0014, REQ_0505

   Wago, Omron, and Beckhoff have shipped ESI files at different
   schema-version baselines. The parser shall track the highest
   shipped schema with the opaque-blob escape hatch
   (:need:`ADR_0074`) catching everything else. A schema-only
   conformance test set (:need:`TEST_0420`) anchors the parser
   against the canonical schema; a real-world fixture set
   (:need:`TEST_0421`) anchors it against actual vendor files.

.. risk:: Generated code becomes load-bearing without migration path
   :id: RISK_0014
   :status: open
   :links: QG_0013

   If many consumers depend on the generated module's struct
   names (e.g. ``EL3001 { pdo: … }``), changing naming policy
   (:need:`REQ_0511`) becomes a breaking change for every
   downstream. Mitigation: lock naming policy under
   ``ethercat-esi-codegen`` (not the backend), version-bump that
   crate per semver on any naming change, document the breaking
   matrix in CHANGELOG.
