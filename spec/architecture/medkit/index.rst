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
   :implements: REQ_0912, REQ_0916

   Transport-neutral read-diagnostic core. Resolves entity-tree queries, fault
   lists, and the worst-wins health rollup over a ``Provider``, independent of
   any HTTP framework. Zero taktora dependencies.

.. building-block:: taktora-medkit-gateway-axum
   :id: BB_0107
   :status: open
   :implements: REQ_0911, REQ_0916

   The HTTP surface: an axum router exposing the gateway over the ros2_medkit
   REST contract, run on a tokio runtime. axum and tokio are not taktora
   dependencies, so this crate remains part of the extractable core. The
   walking skeleton that serves the read families (and ``501`` for deferred
   ones) is a later slice.

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
