.. _medkit-architecture:

Runtime diagnostics (medkit)
============================

Architecture of ``taktora-medkit`` — the SOVD-aligned runtime-diagnostics
surface specified by :need:`FEAT_0100`. This page records the load-bearing
architectural decision (the off-path boundary and extractable-core layout) and
the crate-level building blocks that realise it.

Solution strategy
-----------------

arc42 §4.

.. arch-decision:: Off-path diagnostics boundary + extractable-core layout
   :id: ADR_0111
   :status: open
   :refines: FEAT_0100

   **Context.** taktora's value is a bounded-time control path: the
   taktora-executor WaitSet loop must not allocate or block in steady state
   (:need:`REQ_0104`). A diagnostics surface wants the opposite — a REST
   server, JSON serialisation, an async runtime, unbounded client behaviour.
   Wiring that directly into the runtime would put network and allocation
   latency on the control path. Separately, medkit is a clean-room take on the
   ros2_medkit contract and may later live as its own project; if taktora types
   leak into its model and gateway, that extraction becomes a detangle.

   **Decision.** Quarantine the two concerns by crate boundary. The **core**
   crates — ``taktora-medkit-model`` (wire DTOs), ``taktora-medkit-provider``
   (the data-source seam), ``taktora-medkit-gateway`` (transport-neutral read
   core), ``taktora-medkit-gateway-axum`` (HTTP surface) — carry **zero**
   ``taktora-*`` dependencies and know nothing about the executor or
   connectors. All taktora coupling lives in **binding** crates
   (``taktora-medkit-binding-executor``, ``taktora-medkit-binding-connector``)
   that implement the provider seam by draining non-blocking
   ``Observer`` / ``ExecutionMonitor`` / ``ConnectorHealth`` callbacks into a
   bounded forwarding channel, consumed off the control path on a separate
   tokio runtime and allocator (:need:`REQ_0910`, :need:`REQ_0913`). This
   mirrors the connector framework's own off-path gateway pattern
   (:need:`ADR_0003`, :need:`QG_0001`).

   **Consequences.** ✅ The control path stays bounded — diagnostics can stall
   without perturbing the machine. ✅ The core folder lifts out via
   ``git filter-repo`` (:need:`REQ_0916`); ``cargo tree`` on any core crate
   shows no ``taktora-*`` edge, enforceable in CI. ✅ The provider seam lets the
   same gateway run over a mock, a manifest, or live taktora bindings. ❌ The
   binding crates carry the awkward glue (callback → bounded channel → async
   drain) and must be tested for drop-on-full rather than block-on-full. ❌ The
   model cannot reuse taktora types (e.g. ``ConnectorHealth``); it re-expresses
   them as its own DTOs and the binding maps across the boundary.

.. arch-decision:: Snapshot/merge read seam + shape-diff contract verification
   :id: ADR_0112
   :status: accepted
   :refines: FEAT_0100

   **Context.** The walking skeleton (GitHub #81) must take HTTP in and emit
   contract-correct SOVD JSON out, backed by the mock provider, and prove it
   against ``contract/golden/*.json``. Two design questions fell out. (1) The
   gateway needs a place to assemble the read-model that the later slices plug
   into: #82 applies a manifest, #83/#84 contribute live snapshots from
   bindings. (2) The captured golden corpus is **mutually inconsistent** — it
   was recorded from the upstream binary at different times, so e.g.
   ``function_hosts.json`` lists five apps including ``fault_manager`` while
   ``component_hosts.json`` lists four without it. No single live server state
   can reproduce every fixture byte-for-byte simultaneously.

   **Decision.** (1) Introduce a plain-data ``ProviderSnapshot`` (entities,
   typed relationship edges, faults, data) as the **snapshot contract** the
   ``Provider`` seam produces, and a ``MergePipeline`` that folds snapshots
   (and, later, a manifest) into an indexed ``MergedView``. Pure resolver
   methods on the view produce the wire DTOs; the axum layer is a thin adapter
   that holds an ``Arc<MergedView>`` built once at startup. Relationship items
   carry their context-specific ``x-medkit`` decoration in the snapshot, since
   the producer (mock now, binding later) knows it. (2) Verify the live HTTP
   surface by **structural shape-diffing** against the golden corpus — every
   key, nesting, and value type the contract constrains must be present — rather
   than byte-identity. Byte-for-byte fidelity of the model types stays pinned by
   the model crate's snapshot tests (:need:`TEST_0905`); the gateway test owns
   the wire/transport contract (envelopes, status codes, ``501`` decline).
   Deferred families decline through a single router ``fallback`` that returns a
   contract-shaped ``501``, so any unmatched path is a clean decline, never a
   ``404``.

   **Consequences.** ✅ #82/#83/#84 have an obvious seam: add a snapshot source
   or a manifest step to the pipeline without touching the HTTP layer. ✅ The
   resolvers stay pure and transport-neutral, testable without a socket.
   ✅ Shape-diffing tolerates the corpus's internal inconsistency while still
   catching envelope/casing/structure regressions. ❌ The gateway test does not
   assert exact bytes, so a value-level divergence within a correct shape would
   pass there (the model crate's byte tests cover that axis). ❌ Server-rendered
   views the model does not carry (the single-entity capability catalogue, the
   ``/health`` telemetry blocks) are served best-effort and shape-diffed only
   loosely, a documented gap until a richer provider lands.

Building block view
-------------------

arc42 §5.

The diagnostics surface decomposes into four extractable **core** crates and
two **binding** crates. Core crates depend only on each other and external
crates; binding crates additionally depend on taktora runtime crates and on the
provider seam.

.. building-block:: taktora-medkit-model
   :id: BB_0104
   :status: open
   :implements: REQ_0914, REQ_0915, REQ_0916

   Wire DTOs for the SOVD surface: the entity tree (Area / Component /
   Function / App), the DTC/fault model (status sub-object, severity,
   occurrence count, reporting sources), freeze-frame / snapshot environment
   data, and the reusable collection envelope. ``serde`` only; zero taktora
   dependencies. Byte-for-byte contract alignment against the captured corpus
   is owned by a later slice.

.. building-block:: taktora-medkit-provider
   :id: BB_0105
   :status: open
   :implements: REQ_0913, REQ_0916

   The data-source seam: a ``Provider`` trait the gateway reads through, plus a
   mock provider for tests and the walking skeleton. Zero taktora dependencies;
   live data arrives only via binding crates that implement this trait.

.. building-block:: taktora-medkit-gateway
   :id: BB_0106
   :status: open
   :implements: REQ_0912, REQ_0916, REQ_0917

   Transport-neutral read-diagnostic core. A ``MergePipeline`` folds one or more
   ``ProviderSnapshot``\s (and, later, a manifest) into a ``MergedView``; pure
   resolver methods on the view turn a request into a wire DTO — entity-tree
   queries, relationship sub-resources, fault lists, the single-fault detail,
   data reads, and the worst-wins health rollup — independent of any HTTP
   framework. Zero taktora dependencies.

.. building-block:: taktora-medkit-gateway-axum
   :id: BB_0107
   :status: open
   :implements: REQ_0911, REQ_0916, REQ_0917, REQ_0918, REQ_0919

   The HTTP surface: an axum router exposing the gateway's read-core resolvers
   over the ros2_medkit REST contract on the ``/api/v1`` prefix, run on a tokio
   runtime. Serves the entity tree, relationship sub-resources, fault lists and
   detail, and data reads; answers a contract-shaped ``501`` for deferred
   families via a route fallback; and folds in baseline transport hardening
   (CORS, a token-bucket rate limit, optional TLS behind a ``tls`` feature),
   each configurable with documented defaults. The server holds an
   ``Arc<MergedView>`` built once from the provider snapshot; live-refresh and
   manifest application are downstream slices that do not change the HTTP
   surface. axum and tokio are not taktora dependencies, so this crate remains
   part of the extractable core. A sibling ``taktora-medkit-gateway-axum-tests``
   crate (``publish = false``) hosts the live-server integration and smoke tests
   so the published manifest stays free of internal-crate dev-deps.

.. building-block:: taktora-medkit-binding-executor
   :id: BB_0108
   :status: open
   :implements: REQ_0910, REQ_0913

   Sources liveness and timing from taktora-executor ``Observer`` /
   ``ExecutionMonitor`` hooks and feeds them, off the control path, into a
   ``Provider``. Depends on taktora-executor and the provider seam; the only
   place (with its connector sibling) that taktora types enter the diagnostics
   surface.

.. building-block:: taktora-medkit-binding-connector
   :id: BB_0109
   :status: open
   :implements: REQ_0910, REQ_0912

   Maps connector-framework ``ConnectorHealth`` transitions into SOVD
   Components and DTCs (worst-wins rollup, last-sample freeze-frames), feeding
   the ``Provider`` off the control path. Depends on taktora-connector-core and
   the provider seam.

.. architecture:: medkit crate decomposition
   :id: ARCH_0080
   :status: open
   :refines: BB_0104, BB_0105, BB_0106, BB_0107, BB_0108, BB_0109

   Crate-level building blocks and their dependency edges (depender → dependee).
   The graph is acyclic and the cut between core and binding crates is the
   extraction seam: every edge crossing into ``taktora-*`` originates in a
   binding crate.

   .. mermaid::

      graph TD
        axum[taktora-medkit-gateway-axum] --> gw[taktora-medkit-gateway]
        gw --> prov[taktora-medkit-provider]
        gw --> model[taktora-medkit-model]
        prov --> model
        be[taktora-medkit-binding-executor] --> prov
        be --> exec[taktora-executor]
        bc[taktora-medkit-binding-connector] --> prov
        bc --> conn[taktora-connector-core]
