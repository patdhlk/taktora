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

.. arch-decision:: Per-task atomic sink for the executor hook write path
   :id: ADR_0114
   :status: accepted
   :refines: ADR_0111

   **Context.** The executor binding (GitHub #83) must record App / executor
   liveness and per-task timing from taktora-executor ``Observer`` /
   ``ExecutionMonitor`` hooks. Those hooks run on the executor ``WaitSet`` thread
   inside the bounded-time control path (:need:`REQ_0104`), so the write path
   must not heap-allocate and must not block or contend a lock that could perturb
   the machine (:need:`REQ_0925`). The provider read path, by contrast, runs on
   the gateway's own (tokio) runtime and may allocate freely. The seam between
   them must be wait-free on the producer side. ``ADR_0111`` names a "bounded
   forwarding channel" as the generic mechanism; this decision fixes the concrete
   shape for the executor binding.

   **Decision.** Use a fixed set of **pre-allocated per-task slots**, each a bag
   of atomics (liveness state, lifecycle counters, last / EWMA / min / max
   execution duration, scan period), rather than an overwrite ring of
   observation records. Tasks are registered up front (``with_tasks``) so a hook
   resolves its ``TaskId`` to a slot through a read-only map and folds the
   observation with single-producer relaxed atomic stores — no allocation, no
   lock, no compare-exchange. Because every hook fires from the one ``WaitSet``
   thread, the structure is single-producer / single-consumer: the gateway reads
   the same atomics to build the snapshot. A ring of per-cycle records (à la
   ``taktora-telemetry-export``) was considered but rejected here: the gateway
   wants the *current* folded liveness and timing, not a per-cycle history, so a
   slot that the producer overwrites in place and the reader samples is simpler
   and bounds memory to the task count rather than a backlog depth. The trade is
   that an unregistered task's observations are dropped (counted as ignored)
   rather than allocated for.

   **Consequences.** ✅ The hook path is provably allocation-free and lock-free,
   asserted by a counting-allocator differential test (:need:`TEST_0914`).
   ✅ Memory is bounded to the registered task count, fixed at construction.
   ✅ A stalled or slow gateway reader can never back-pressure or perturb the
   control path. ❌ Tasks must be known up front; an item whose ``task_id`` was
   not registered is invisible to diagnostics (by design — no control-path
   allocation to grow the set). ❌ In-place overwrite means the gateway sees only
   the latest folded values, not a per-cycle trace; a richer history would need
   the ring pattern and is out of scope for this slice.

.. arch-decision:: Mandatory grouping manifest, applied in the merge pipeline
   :id: ADR_0113
   :status: accepted
   :refines: FEAT_0100

   **Context.** medkit v1 does no service discovery (raw iceoryx2 introspection
   is out of scope), so nothing enumerates the system to supply the
   Area/Component grouping the SOVD tree hangs on. The bindings emit only flat,
   raw entities (``app:<task>``, ``component:<subdevice>``) with no notion of
   which Area they belong to or how Components nest, so the relationship
   sub-resources (``/areas/{id}/components``, ``/components/{id}/hosts``) would
   have nothing to return. The grouping has to come from somewhere declared, and
   it must be both programmable (tests, in-code wiring) and ops-editable (no
   recompile to re-topologise). It must also not become a hard precondition that
   bricks a fresh deployment that has not authored one yet.

   **Decision.** Introduce a sibling core crate ``taktora-medkit-manifest``
   (:need:`BB_0110`) holding the grouping as plain data over two surfaces that
   build one identical ``Manifest`` value — a type-safe builder and a TOML loader
   over a committed ``medkit.toml`` — pinned equal by a test so they never drift.
   The manifest owns the binding id conventions (``app:`` / ``component:``
   prefixes) via ``parent_of`` and emits the declared skeleton as model entities;
   it carries zero ``taktora-*`` deps (``serde`` + ``toml`` over the model DTOs),
   so it does not contend with the binding crates and stays inside the
   extractable core. Application of the manifest lives in the ``MergePipeline``
   (the seam :need:`ADR_0112` reserved for it), **not** in the provider: the
   pipeline injects the declared entities, re-parents the raw entities, and
   synthesises the relationship edges (``components`` / ``contains`` / ``hosts``
   / ``subcomponents``) from the resulting hierarchy. A missing or empty manifest
   is a no-op fold, falling back to flat grouping rather than erroring. The axum
   ``GatewayConfig`` surfaces an optional ``manifest`` so ops attach a loaded
   ``medkit.toml`` without touching the HTTP layer.

   **Consequences.** ✅ The grouping is declarative and lives in one place; ops
   edit ``medkit.toml`` while tests wire the same shape through the builder.
   ✅ Re-parenting in the pipeline keeps the provider seam a dumb data source and
   leaves the bindings (#83/#84) free of grouping concerns. ✅ The flat fallback
   means the manifest is mandatory *for grouping*, not for serving — a
   manifest-less deployment still answers the read-core. ❌ The manifest restates
   topology the running system already half-knows, and a stale ``medkit.toml``
   silently mis-groups (a declared parent that never appears just hosts nothing).
   ❌ The pipeline must know the ``app:`` / ``component:`` id conventions to pick
   the relation type, coupling it loosely to the bindings' id scheme.

.. arch-decision:: Connector health → DTC mapping and last-sample freeze-frame
   :id: ADR_0115
   :status: accepted
   :refines: FEAT_0100

   **Context.** The connector binding (GitHub #84) must turn a connector's
   ``ConnectorHealth`` transition stream into SOVD Components and DTCs. Three
   shapes had to be pinned. (1) ``taktora-connector-core`` exposes health
   per-connector and has **no** ``subscribe_health()``; its states carry reasons
   as **strings** (``Degraded{reason}``), not a typed fault enum, and stamp
   transitions with a monotonic ``Instant`` that cannot express wall-clock
   occurrence timestamps. (2) The ``Provider`` seam is read with ``&self`` from
   the gateway's request path, while health events arrive on the connector's
   off-path drain — writer and reader are concurrent. (3) The captured contract
   carries a freeze-frame per confirmed DTC, but ``ProviderSnapshot`` models
   only ``FaultSummary`` (no freeze-frame field) and the gateway's best-effort
   ``fault_detail`` emits an empty ``snapshots`` array.

   **Decision.** (1) Model the input as a health **event stream** the binding
   ingests (``on_health_event`` / ``apply``), pairing each event with a
   wall-clock epoch timestamp the drain supplies. Map ``Down`` → a Critical
   ``FIELDBUS_NOT_OPERATIONAL`` DTC and ``Degraded`` → a Warning
   ``FIELDBUS_DEGRADED`` DTC carrying the reason string read off the variant;
   ``Connecting`` keeps the prior fault active (recovery in flight) and ``Up``
   heals. Keep confirmed DTCs in memory across heals (UDS-style) so occurrence
   counts and first/last occurrence accumulate; the Component's reported health
   is the worst of the bare state and any active DTC. (2) Hold the DTC store
   behind interior mutability (a ``Mutex``) so the callback writes and the
   gateway reads through one consistent lock. (3) Confirm on the first callback
   sample (no multi-cycle pending window in v1) and capture the **last hook
   sample** as the freeze-frame — or, absent a sample, a synthesized snapshot of
   the health condition — rendered under the contract's ``snapshots`` /
   ``extended_data_records`` shape. Because the gateway cannot carry rich
   freeze-frames through ``ProviderSnapshot.faults``, also surface each DTC's
   environment data under the Component's ``data`` resource so the freeze-frame
   is reachable through the running gateway.

   **Consequences.** ✅ The binding is testable with a simulated transition
   sequence and pluggable onto a real per-connector health surface later.
   ✅ Reason strings flow through unchanged, so new degraded conditions need no
   binding change. ✅ DTC memory gives a maintenance history (occurrence counts,
   heal/raise) rather than a momentary view. ❌ A wall-clock timestamp must be
   supplied alongside each event, since the connector's ``Instant`` is not
   convertible to epoch time. ✅ The freeze-frame now also reaches HTTP clients
   through the proper fault-detail ``snapshots`` array — :need:`ADR_0116` adds an
   additive ``ProviderSnapshot.fault_environments`` seam the gateway's
   ``fault_detail`` sources from, resolving the gap this decision documented (the
   ``data`` resource is retained as extra exposure). ❌ Single-sample
   confirmation means no ``pendingDTC`` window in v1.

.. arch-decision:: Additive freeze-frame seam through the snapshot (fault_environments)
   :id: ADR_0116
   :status: accepted
   :refines: FEAT_0100

   **Context.** :need:`ADR_0115` left a documented gap: ``ProviderSnapshot``
   carried only ``FaultSummary`` per fault (no environment data), so the
   gateway's ``fault_detail`` (``…/faults/{fault_code}``) hard-coded an empty
   ``snapshots`` array and empty ``extended_data_records``. The connector binding
   already computes the full ``EnvironmentData`` freeze-frame at confirmation but
   could only surface it through a ``…/data`` workaround, not the contract's
   proper fault-detail endpoint. The fix must not reshape the ``FaultSummary``
   wire contract or the fault-list path, on which other consumers depend.

   **Decision.** Carry per-fault environment data through the snapshot seam
   **additively**: add ``ProviderSnapshot.fault_environments`` (``entity_id`` →
   ``fault_code`` → ``EnvironmentData<Value>``), defaulting empty, plus a
   ``Provider::fault_environment`` accessor defaulting to ``None``. The default
   ``snapshot()`` leaves the map empty, so existing providers (the executor
   binding and mock) are unaffected and the ``FaultSummary`` fault-list output is
   byte-for-byte unchanged. ``MergedView`` retains the merged map through the
   fold; ``fault_detail`` looks up ``(entity, fault_code)`` and substitutes the
   real ``EnvironmentData`` when present, falling back to the occurrence-only
   shape otherwise. The connector binding populates ``fault_environments`` from
   the freeze-frame it already computes, so a confirmed DTC's
   ``…/faults/{fault_code}`` now returns the freeze-frame under the contract's
   ``snapshots`` / ``extended_data_records``. See :need:`REQ_0929`,
   :need:`TEST_0918`.

   **Consequences.** ✅ The freeze-frame reaches clients through the proper SOVD
   fault-detail endpoint, closing the :need:`ADR_0115` gap. ✅ The change is
   purely additive — the fault-list wire contract and every existing provider
   compile and behave unchanged. ✅ The ``…/data`` exposure is retained as an
   extra surface rather than a workaround. ❌ The snapshot seam carries a second
   per-fault map alongside ``faults``; a binding that captures freeze-frames must
   populate both, keyed consistently by fault code.

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
   :implements: REQ_0913, REQ_0916, REQ_0929

   The data-source seam: a ``Provider`` trait the gateway reads through, plus a
   mock provider for tests and the walking skeleton. Zero taktora dependencies;
   live data arrives only via binding crates that implement this trait.

.. building-block:: taktora-medkit-gateway
   :id: BB_0106
   :status: open
   :implements: REQ_0912, REQ_0916, REQ_0917, REQ_0929

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

.. building-block:: taktora-medkit-manifest
   :id: BB_0110
   :status: implemented
   :implements: REQ_0920, REQ_0921, REQ_0922, REQ_0916

   The mandatory Area/Component grouping manifest: a type-safe builder core and a
   TOML loader (over a committed ``medkit.toml``) that build one identical
   ``Manifest`` value, plus the declared-skeleton entities and the
   ``parent_of`` re-parent lookup the merge pipeline consumes. ``serde`` + ``toml``
   over the model DTOs; zero taktora dependencies. The ``MergePipeline``
   (:need:`BB_0106`) applies it and the axum ``GatewayConfig`` (:need:`BB_0107`)
   surfaces it; an empty/absent manifest falls back to flat grouping.

.. building-block:: taktora-medkit-binding-executor
   :id: BB_0108
   :status: implemented
   :implements: REQ_0910, REQ_0913, REQ_0923, REQ_0924, REQ_0925

   Sources liveness and timing from taktora-executor ``Observer`` /
   ``ExecutionMonitor`` hooks and feeds them, off the control path, into a
   ``Provider``. Depends on taktora-executor and the provider seam; the only
   place (with its connector sibling) that taktora types enter the diagnostics
   surface.

   The lifecycle hooks (``on_app_start`` / ``on_app_stop`` / ``on_app_error``
   plus the executor-level up / down / fault hooks) fold App and executor
   liveness, and ``post_execute`` / ``on_cycle_stats`` fold per-task timing (an
   EWMA latency and a period / rate analog), into a bounded, pre-allocated,
   per-task atomic sink. Because the hooks fire only from the single ``WaitSet``
   thread the sink is single-producer / single-consumer, so the write path
   neither allocates nor locks (:need:`ADR_0114`, :need:`REQ_0925`). The
   ``Provider`` read path reads those atomics on the gateway's runtime and emits
   raw entities (``app:<task>`` plus a synthetic executor entity) with their
   health and a readable ``data`` tree (:need:`REQ_0923`, :need:`REQ_0924`).
   Tests live in the ``taktora-medkit-binding-executor-tests`` sibling
   (``publish = false``) so the published manifest carries no internal dev-deps.

.. building-block:: taktora-medkit-binding-connector
   :id: BB_0109
   :status: implemented
   :implements: REQ_0910, REQ_0912, REQ_0926, REQ_0927, REQ_0928
   :links: TEST_0915, TEST_0916, TEST_0917

   Maps connector-framework ``ConnectorHealth`` transitions into SOVD
   Components and DTCs (worst-wins rollup, last-sample freeze-frames), feeding
   the ``Provider`` off the control path. Depends on taktora-connector-core and
   the provider seam.

   The binding is a stateful ``MedkitProvider``: it ingests a connector's health
   event stream through ``on_health_event`` / ``apply`` (taktora-connector-core
   exposes health per-connector with no ``subscribe_health()``, so the input is
   modelled as an event stream a real per-connector surface drives and tests
   drive with a simulated sequence), maintains a DTC store behind interior
   mutability — so a callback can write while the gateway reads the ``Provider``
   with ``&self`` — and renders it into a ``ProviderSnapshot`` of one raw
   Component plus its DTCs. ``Down`` raises a Critical
   ``FIELDBUS_NOT_OPERATIONAL`` DTC; ``Degraded`` a Warning ``FIELDBUS_DEGRADED``
   carrying the reason string. DTC memory persists confirmed DTCs across heals
   (UDS-style), tracking occurrence count and first/last occurrence. The
   confirmed-time freeze-frame — the last hook sample, or a synthesized health
   snapshot — is rendered under the contract's ``snapshots`` /
   ``extended_data_records`` shape and also surfaced under the Component's
   ``data`` resource so it is reachable through the running gateway. See
   :need:`ADR_0115`.

.. architecture:: medkit crate decomposition
   :id: ARCH_0080
   :status: open
   :refines: BB_0104, BB_0105, BB_0106, BB_0107, BB_0108, BB_0109, BB_0110

   Crate-level building blocks and their dependency edges (depender → dependee).
   The graph is acyclic and the cut between core and binding crates is the
   extraction seam: every edge crossing into ``taktora-*`` originates in a
   binding crate.

   .. mermaid::

      graph TD
        axum[taktora-medkit-gateway-axum] --> gw[taktora-medkit-gateway]
        axum --> manifest[taktora-medkit-manifest]
        gw --> prov[taktora-medkit-provider]
        gw --> model[taktora-medkit-model]
        gw --> manifest
        manifest --> model
        prov --> model
        be[taktora-medkit-binding-executor] --> prov
        be --> exec[taktora-executor]
        bc[taktora-medkit-binding-connector] --> prov
        bc --> conn[taktora-connector-core]
