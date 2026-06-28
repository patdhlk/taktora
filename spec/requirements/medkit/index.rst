Runtime diagnostics (SOVD-aligned)
==================================

``taktora-medkit`` is a runtime-diagnostics surface for taktora: it presents
the running system as a `SOVD <https://www.asam.net/standards/detail/sovd/>`_
-aligned entity tree (Area / Component / Function / App) carrying a DTC/fault
model with freeze-frames, and serves it over a REST surface that is **drop-in
compatible** with the wire contract of the C++ project ``selfpatch/ros2_medkit``.

It is a clean-room Rust take on that diagnostic *contract* — not a port of its
ROS 2 internals. Where ros2_medkit reads a ROS 2 graph, taktora-medkit sources
its model from taktora's own runtime (connector health, executor timing)
through non-blocking, **off-the-control-path** hooks, so diagnostics can never
perturb the bounded-time WaitSet path that drives the machine.

This umbrella is a peer of :need:`FEAT_0010` "PLC runtime heart" and
:need:`FEAT_0030` "Connector framework"; medkit is a general-purpose
diagnostics mechanism layered on the taktora runtime, not bound to any one
protocol or to the PLC use case.

.. feat:: Runtime diagnostics (SOVD-aligned)
   :id: FEAT_0100
   :status: open

   A runtime-diagnostics surface that models the live taktora system as a
   SOVD-aligned entity tree (Area / Component / Function / App) with a
   DTC/fault model (status sub-object, occurrence counts, reporting sources,
   freeze-frames / snapshots), a worst-wins health rollup across the tree, and
   a REST surface that is drop-in compatible with the ros2_medkit wire
   contract. The diagnostics surface attaches to taktora through non-blocking
   callback hooks only and runs off the control path, on its own runtime and
   allocator.

   The crates are split so the diagnostic *model*, *provider* seam, and
   *gateway* carry zero taktora dependencies and can be extracted as a
   standalone project, with all taktora coupling quarantined in ``-binding-*``
   crates (see :need:`ADR_0111`).

Requirements
------------

.. req:: Off-path / freedom from interference
   :id: REQ_0910
   :status: open
   :satisfies: FEAT_0100

   The diagnostics gateway shall never execute inside taktora-executor's
   bounded-time WaitSet path. It shall attach only through non-blocking
   ``Observer`` / ``ExecutionMonitor`` / ``ConnectorHealth`` callbacks that
   hand work to a bounded forwarding channel drained by a separate tokio
   runtime, so a slow, stalled, or backlogged diagnostics consumer cannot
   block, delay, or allocate on the control path. Forwarding under a full
   channel shall drop, not block.

.. req:: Drop-in client compatibility
   :id: REQ_0911
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0104, TEST_0900, TEST_0905

   A diagnostic client written against the ros2_medkit REST contract shall
   work unchanged against the taktora-medkit backend: the served JSON shapes
   (field names, casing, collection envelope, DTC status sub-object,
   freeze-frame structure) shall match the captured contract corpus for every
   family v1 serves. Divergence from the corpus shall be a failing test, not a
   field report.

.. req:: Worst-wins health rollup
   :id: REQ_0912
   :status: open
   :satisfies: FEAT_0100

   Each entity's aggregated health shall be the worst (most severe) health of
   itself and all entities it contains. Rolling a child into a fault state
   shall roll its ancestors at least to that state; clearing the last faulting
   child shall be required before an ancestor can return to healthy.

.. req:: Callback-hooks-only attach in v1
   :id: REQ_0913
   :status: open
   :satisfies: FEAT_0100

   v1 shall source its model exclusively from in-process taktora callback
   hooks. It shall not attach over iceoryx2 shared memory and shall not stand
   up its own iceoryx2 node; a shared-memory attach path is explicitly
   deferred to a later revision.

.. req:: SOVD entity-tree model
   :id: REQ_0914
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0104, TEST_0900, TEST_0905

   The model shall represent the system as a tree of typed entities — Area,
   Component, Function, App — each carrying a stable id, a human-readable
   name, its place in the hierarchy, and the diagnostic capabilities it
   exposes, matching the SOVD entity collections of the wire contract.

.. req:: DTC / fault model with freeze-frames
   :id: REQ_0915
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0104, TEST_0900, TEST_0905

   A fault shall be modelled as a DTC carrying a fault code, a SOVD/UDS-style
   status sub-object, severity, occurrence count, the set of reporting
   sources, and environment data — first/last occurrence records plus zero or
   more freeze-frame / snapshot captures of the system state at fault time.

.. req:: Extractable diagnostic core
   :id: REQ_0916
   :status: open
   :satisfies: FEAT_0100

   The core crates — model, provider seam, gateway, and HTTP gateway — shall
   carry **zero** ``taktora-*`` dependencies, so the diagnostics folder can be
   lifted out into a standalone repository via ``git filter-repo`` rather than
   detangled. All coupling to taktora shall live in dedicated ``-binding-*``
   crates that depend on the core through the provider seam only.

.. req:: Read-diagnostic core HTTP surface
   :id: REQ_0917
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0107, TEST_0906

   The gateway shall serve the SOVD read-diagnostic core over HTTP on the
   ``/api/v1`` prefix, backed by the ``Provider`` seam: the entity tree
   (areas / components / apps / functions, each with its single-entity view and
   the relationship sub-resources ``contains`` / ``components`` /
   ``subcomponents`` / ``hosts`` / ``depends-on`` / ``is-located-on`` /
   ``belongs-to``), fault lists (global and entity-scoped, with the ``status``
   filter) and the single-fault detail, and readable ``data``. Each served body
   shall carry the contract collection / fault / error envelope shape, so a
   client written against the ros2_medkit contract reads them unchanged.

.. req:: Deferred families decline with a contract-shaped 501
   :id: REQ_0918
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0107, TEST_0907

   For the families v1 does not implement — operations, configuration writes,
   bulk-data, locks, scripts, updates / OTA, triggers, cyclic-subscriptions,
   logs, status actions, auth, and the ``x-medkit-*`` vendor endpoints — the
   gateway shall answer ``501 Not Implemented`` with a contract-shaped
   ``GenericError`` body, never a ``404`` or a parse error. A path-hardcoding
   client shall therefore receive a clean, documented decline rather than a
   route miss.

.. req:: Baseline transport hardening, off the control path
   :id: REQ_0919
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0107, TEST_0908

   The HTTP surface shall offer configurable CORS, a token-bucket rate limit,
   and optional TLS, each with a documented default (permissive CORS, rate
   limit disabled, TLS disabled, bind ``127.0.0.1:8080``). These run only on the
   diagnostics server's own runtime and never on taktora's bounded-time control
   path, preserving the off-path boundary of :need:`ADR_0111`.

.. req:: Executor liveness and timing from the hook seam
   :id: REQ_0923
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0108, TEST_0913

   The executor binding shall implement the taktora-executor ``Observer`` and
   ``ExecutionMonitor`` traits and register through the executor builder. From
   the lifecycle hooks (``on_app_start`` / ``on_app_stop`` / ``on_app_error``
   and the executor-level ``on_executor_up`` / ``on_executor_down`` /
   ``on_executor_fault``) it shall derive App and executor entity liveness and
   ``HealthState``; from ``post_execute`` it shall roll per-task execution
   timing (an EWMA latency analog) and from ``on_cycle_stats`` the scan period
   (a rate / Hz analog).

.. req:: Executor binding exposed through the provider seam
   :id: REQ_0924
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0108, TEST_0912, TEST_0913

   The binding shall expose the recorded liveness, health, and timing to the
   gateway through the ``Provider`` seam: raw entities (``app:<task>`` plus a
   synthetic executor entity), per-entity health, and a readable ``data`` tree
   carrying the liveness and timing values. Entities shall be emitted raw so the
   manifest (when present) can place them and the binding still works flat
   without it. The read path shall run on the gateway's own runtime, off the
   control path.

.. req:: Allocation-free, non-blocking hook write path
   :id: REQ_0925
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0108, TEST_0914

   The hooks run on the executor ``WaitSet`` thread inside the bounded-time
   control path, so the write path shall perform **no heap allocation** and
   shall take **no lock** that could contend the control path. It shall write
   into a bounded, pre-allocated, single-producer / single-consumer structure
   (per-task atomics) so a stalled or slow diagnostics reader can never perturb
   the machine, holding the freedom-from-interference contract of
   :need:`ADR_0111` (see :need:`ADR_0114`).

.. req:: Mandatory Area/Component grouping manifest
   :id: REQ_0920
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0110, TEST_0909

   Because v1 does no service discovery, the Area/Component grouping shall come
   from a manifest, supplied over two surfaces that build one identical value: a
   type-safe builder core (``Manifest::builder().area(..).component(..)
   .map_task(..).map_subdevice(..).build()``) for tests and programmatic wiring,
   and a TOML loader (``Manifest::from_toml``) deserialising the same shape from a
   committed example ``medkit.toml`` so ops can edit topology without
   recompiling. The manifest crate shall carry zero ``taktora-*`` dependencies.

.. req:: Merge pipeline applies the manifest
   :id: REQ_0921
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0110, TEST_0910

   Folding the read-model through a non-empty manifest shall materialise the
   declared Areas and Components as entities and re-parent the binding-emitted
   raw entities (``app:<task>``, ``component:<subdevice>``) under them per the
   mapping rules, so that ``GET /api/v1/areas/{id}/components`` and the
   component-nesting sub-resources (``…/hosts``, ``…/subcomponents``) return the
   declared structure. The re-parenting shall live in the merge pipeline, not in
   the provider seam.

.. req:: Empty or absent manifest falls back to flat grouping
   :id: REQ_0922
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0110, TEST_0911

   A missing or empty manifest shall not be an error: the pipeline shall fall
   back to the flat provider grouping (the pre-manifest behaviour) without
   panicking, so a deployment that has not yet authored a ``medkit.toml`` still
   serves the read-core.

.. req:: Connector health maps to a SOVD Component and DTCs
   :id: REQ_0926
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0109, TEST_0915

   The connector binding shall present each connector as a SOVD Component (the
   bridge / ``SubDevice`` standing in for it) and map its ``ConnectorHealth``
   transitions to DTCs: a ``Down`` connector shall raise a Critical
   ``FIELDBUS_NOT_OPERATIONAL`` DTC, and a ``Degraded`` connector shall enter a
   Warning health state and raise a ``FIELDBUS_DEGRADED`` DTC carrying the
   reason string. The reason shall be read as a string off the health variant;
   the binding shall not depend on a typed fault enum. The Component shall be
   emitted raw (no placement), so the manifest can place it when present and it
   works flat without one. The Component's reported health shall be the worst of
   the bare health state and any active DTC.

.. req:: DTC lifecycle and occurrence bookkeeping
   :id: REQ_0927
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0109, TEST_0916

   Across repeated health transitions the binding shall maintain per-DTC
   lifecycle state: the SOVD/UDS status bits (``testFailed`` while the condition
   is present, ``confirmedDTC`` latched once confirmed), an occurrence count
   incremented each time a cleared DTC is re-raised, and first/last occurrence
   timestamps. A return to ``Up`` shall heal active DTCs — clearing
   ``testFailed`` and rolling the Component back to healthy — while keeping the
   DTC in memory (confirmed) as maintenance history rather than erasing it.

.. req:: Last-sample freeze-frame at confirmation
   :id: REQ_0928
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0109, TEST_0917

   Each confirmed DTC shall carry a freeze-frame captured at confirmation time
   under the contract's ``snapshots`` / ``extended_data_records`` shape. In v1
   (callback-hooks-only, no iceoryx2 PDI slice — :need:`REQ_0913`) the
   freeze-frame shall be the last connector hook sample observed before
   confirmation, or, absent any sample, a synthesized snapshot of the health
   condition (state and reason).

.. req:: Freeze-frame surfaced through the SOVD fault-detail endpoint
   :id: REQ_0929
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0106, BB_0105, TEST_0918

   A fault's freeze-frame environment data shall be reachable through the proper
   SOVD fault-detail endpoint (``…/faults/{fault_code}``), carried under the
   contract's ``snapshots`` / ``extended_data_records`` shape, and not only
   through a ``…/data`` workaround. The snapshot seam shall carry per-fault
   environment data additively (:need:`ADR_0116`): a binding that captures
   freeze-frames shall populate it, while bindings that capture none, and the
   ``FaultSummary`` fault-list wire shape, shall be unchanged. When no
   environment data is carried for a fault, the detail shall fall back to the
   occurrence-only environment shape.

.. req:: Off-path refresh-and-diff loop
   :id: REQ_0930
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0111, TEST_0920

   The gateway shall run a refresh-and-diff loop on the off-path tokio runtime
   that re-polls and re-merges the provider snapshot on a configurable cadence,
   hot-swapping the served ``MergedView`` so the read-core stays live, and diffs
   each new view against the previous one to derive change events. The loop shall
   run off the request/control path so a slow or absent diagnostics reader can
   never perturb polling, holding the freedom-from-interference contract of
   :need:`ADR_0111`.

.. req:: Diff-derived fault change events
   :id: REQ_0931
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0111, TEST_0920

   Diffing two successive merged views shall emit a ``fault_raised`` event for an
   ``(entity, fault_code)`` newly present in the later view and a
   ``fault_cleared`` event for one that vanished. Each event shall carry the
   golden fault-stream payload shape (:need:`REQ_0911`): ``event_type``, the full
   ``fault`` sub-object, a ``timestamp``, and the ``x-medkit`` scoping
   (``entity_id``, ``entity_type``). The event vocabulary is taktora's
   diff-derived set, not the captured ``fault_confirmed`` label (:need:`ADR_0117`).

.. req:: Health-transition change events
   :id: REQ_0932
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0111, TEST_0921

   When an entity's worst-wins health level changes between two successive merged
   views, the loop shall emit a ``health_changed`` event scoped to that entity.
   To preserve the uniform golden frame shape the event shall carry a
   representative ``fault`` sub-object — the worst current fault, or the
   just-cleared fault when health returns to ``OK`` (:need:`ADR_0117`).

.. req:: Trigger subscription surface
   :id: REQ_0933
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0111, TEST_0919

   The gateway shall expose a basic subscription surface under
   ``/api/v1/triggers``: ``POST`` registers a trigger, ``GET`` lists triggers,
   ``GET /{id}`` fetches one, and ``DELETE /{id}`` removes one (a contract-shaped
   ``trigger-not-found`` ``404`` for an unknown id). A trigger shall carry a
   **minimal** filter — by entity id and/or a severity floor — and nothing more.
   Rich condition predicates (data-value thresholds, debounce, boolean
   composition) are explicitly deferred to issue #87 and shall not be implemented
   in this slice.

.. req:: SSE event stream framed per the captured contract
   :id: REQ_0934
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0111, TEST_0920

   ``GET /api/v1/triggers/events`` shall stream the change events as
   Server-Sent Events, delivering only events matching at least one registered
   trigger. Each frame shall match the captured golden
   (``contract/golden/faults_stream_sse_sample.txt``) shape — ``id: <n>`` /
   ``event: <event_type>`` / ``data: <json>`` followed by a blank line — and the
   ``data`` object shall be the golden fault-stream payload so a drop-in
   ``ros2_medkit`` client parses the stream unchanged (:need:`ADR_0117`).

Requirements at a glance
------------------------

.. needtable::
   :columns: id, title, status, satisfies
   :show_filters:
   :filter: "FEAT_0100" in satisfies
