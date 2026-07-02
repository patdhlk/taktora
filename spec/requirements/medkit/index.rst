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

.. req:: Non-blocking, bounded hook write path
   :id: REQ_0925
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0108, TEST_0913

   The hooks run on the executor ``WaitSet`` thread inside the bounded-time
   control path, so the write path shall take **no lock** that could contend
   the control path and shall write into a bounded, pre-allocated,
   single-producer / single-consumer structure (per-task atomics) so a
   stalled or slow diagnostics reader can never perturb the machine, holding
   the freedom-from-interference contract of :need:`ADR_0111` (see
   :need:`ADR_0114`). Heap-allocation-freedom remains the design intent —
   ADR_0114's per-task atomic sink allocates nothing by construction — but
   is **not a verified requirement pre-1.0**: zero-alloc test enforcement is
   scoped to executor and connector scope (:need:`ADR_0133`), and the
   counting-allocator test formerly holding this clause (:need:`TEST_0914`)
   is retired.

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

.. req:: Auth-light token endpoints preserve the client login flow
   :id: REQ_0935
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0112, ADR_0118, TEST_0922

   The gateway shall expose the SOVD authentication endpoints a drop-in client
   calls before it reads any diagnostics: ``POST /api/v1/auth/token`` (singular
   ``token``), ``POST /api/v1/auth/authorize``, and ``POST /api/v1/auth/revoke``,
   POST-only, carved out from under the deferred-family ``501`` fallback. A
   ``client_credentials`` request to the token endpoint shall return an HTTP
   ``200`` carrying a contract-shaped ``AuthTokenResponse`` with the required
   fields ``access_token``, ``token_type`` (``"Bearer"``), ``expires_in``, and
   ``scope``, so a ``client_credentials`` login completes.

.. req:: Permissive dev-mode authenticator is the default
   :id: REQ_0936
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0112, ADR_0118, TEST_0922

   The default authenticator shall be permissive (dev mode): any credentials
   succeed at the token endpoint, and any or no ``Bearer`` token is accepted. The
   issued ``access_token`` shall be a **shape-valid** JWT — three non-empty
   ``base64url``-encoded, dot-separated segments
   (``header.payload.signature``) — but not cryptographically signed. Real JWT
   signing and validation are deferred to tracking issue #87.

.. req:: Authentication flows through a substitutable Authenticator seam
   :id: REQ_0937
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0112, ADR_0118, TEST_0924

   Token issuance and bearer verification shall flow through a single
   ``Authenticator`` trait seam. A strict implementation (real JWT validation +
   RBAC, tracking issue #87) shall be substitutable for the permissive default
   without modifying any request handler — the read-core handlers shall not
   reference the authenticator, and the seam shall be exercisable from outside
   the crate.

.. req:: Resource routes run enforcement = none in v1
   :id: REQ_0938
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0112, ADR_0118, TEST_0923

   Resource (read-core) routes shall run enforcement = none in v1: a presented
   ``Bearer`` token shall be accepted and never verified, and a request shall
   pass auth whether or not it carries a token. The gateway shall never reject a
   resource request on authentication grounds in v1. The enforcement modes
   (``none`` / ``write`` / ``all``) and their RBAC checks are deferred to
   tracking issue #87, behind the :need:`REQ_0937` seam.

.. req:: Full client login-to-read flow over the live gateway
   :id: REQ_0939
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0112, ADR_0118, TEST_0924

   A drop-in client shall be able to complete the full shape against a live
   gateway: ``POST`` ``client_credentials`` to ``/api/v1/auth/token``, obtain the
   ``access_token``, and call a read-core endpoint presenting that token as a
   ``Bearer`` credential, receiving an HTTP ``200``.

.. req:: Diagnostic lock lifecycle — acquire, extend, release
   :id: REQ_0940
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0113, ADR_0120, TEST_0925

   The gateway shall expose SOVD diagnostic-scoped exclusive access on the entity
   kinds the contract defines ``/locks`` for (apps, components):
   ``POST /api/v1/{entity}/{id}/locks`` shall acquire a lock and return HTTP
   ``201`` with a contract-shaped ``Lock`` (``id``, ``owned``, an absolute RFC3339
   ``lock_expiration``, optional ``scopes``); ``PUT .../locks/{lock_id}`` shall
   extend it and return ``204``; ``DELETE .../locks/{lock_id}`` shall release it
   and return ``204``. The acquire body ``AcquireLockRequest`` carries
   ``lock_expiration`` as a millisecond TTL from now; the response renders the
   expiry as an absolute RFC3339 instant.

.. req:: Lock TTL expiry auto-releases
   :id: REQ_0941
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0113, ADR_0120, TEST_0925

   A lock shall auto-release once its TTL elapses: after expiry the resource shall
   be freely re-acquirable by any client without an explicit release. TTL shall be
   evaluated against an injectable wall-clock source so expiry is deterministic
   and testable without sleeping on real time.

.. req:: break_lock supervisor override
   :id: REQ_0942
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0113, ADR_0120, TEST_0926

   A ``break_lock: true`` acquire shall evict a lock currently held by another
   client (supervisor override) and grant the lock to the requester, returning
   HTTP ``201``.

.. req:: X-Client-Id lock ownership
   :id: REQ_0943
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0113, ADR_0120, TEST_0927

   The required ``X-Client-Id`` header (1–256 chars) shall identify the lock
   holder. A second client acquiring a live lock without ``break_lock`` shall
   receive HTTP ``409``; only the holder shall extend or release its lock — a
   non-owner attempt on a live lock shall receive ``409`` — and a missing or
   out-of-range ``X-Client-Id`` shall receive ``400``.

.. req:: Locks are diagnostic-coordination-only QM metadata
   :id: REQ_0944
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0113, ADR_0119, ADR_0120, TEST_0927

   The lock registry shall be in-memory and off the control path, guarding no
   safety-critical resource and adding no edge to the executor/connector binding
   crates or the taktora runtime (preserving the extractable core,
   :need:`REQ_0916`). Locks shall coordinate diagnostic clients against each other
   only; the moment a lock guards an SC resource, the write-surface safety gate
   (:need:`ADR_0119`) applies.

Wire-compatibility parity pass (Tier A)
---------------------------------------

The fixes below close gaps *inside* the already-served surface — places a
path/field-hardcoding ``ros2_medkit`` client would break even though the family
is nominally implemented (:need:`ADR_0125`). They add no write to a
safety-critical resource (:need:`ADR_0119` is untouched).

.. req:: Global fault SSE stream
   :id: REQ_0961
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0120, ADR_0125, TEST_0942

   The gateway shall serve the contract's canonical **global** fault event
   stream at ``GET /api/v1/faults/stream`` as Server-Sent Events, emitting every
   change event unfiltered in the captured golden frame shape
   (``contract/golden/faults_stream_sse_sample.txt``). The trigger-filtered
   stream remains at ``/api/v1/triggers/events``.

.. req:: Entity-scoped triggers
   :id: REQ_0962
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0111, ADR_0125, TEST_0941

   The gateway shall expose triggers per entity at
   ``/{collection}/{id}/triggers`` (list, create, get, update, delete) plus the
   per-trigger SSE stream at ``…/{trigger_id}/events``, for every entity kind. A
   trigger created under an entity shall be pinned to that entity, the
   entity-scoped list shall return only that entity's triggers, and a fetch of
   one entity's trigger under another entity shall be ``404``.

.. req:: Lock read endpoints
   :id: REQ_0963
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0113, ADR_0120, TEST_0940

   The gateway shall serve ``GET /{collection}/{id}/locks`` (list) and
   ``GET …/locks/{lock_id}`` (detail) on the lock-bearing kinds (apps,
   components). ``X-Client-Id`` shall be optional on a read and determine only
   the ``owned`` flag; an expired lock shall be omitted and an unknown lock id
   shall be ``404``.

.. req:: Global fault clear-all
   :id: REQ_0964
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0107, ADR_0119, ADR_0125, TEST_0938

   The gateway shall answer ``DELETE /api/v1/faults`` with ``204``. As with the
   per-entity fault ``DELETE``, the read-only skeleton acknowledges the
   clear-all without mutating state; a real write-through lands with the binding
   write-path under the :need:`ADR_0119` gate.

.. req:: Honest capability advertisement
   :id: REQ_0965
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0106, ADR_0125, TEST_0936

   The root document (``GET /api/v1/``) shall advertise a capability flag as
   ``true`` exactly when the gateway mounts that family's routes, and its
   endpoint catalogue shall list the served vendor extensions (the global
   stream, clear-all, triggers, locks, auth). A capability-gating client shall
   thereby neither skip a working family nor probe a deferred one.

.. req:: SSE keep-alive and reconnect replay
   :id: REQ_0966
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0120, ADR_0125, TEST_0942

   Every SSE endpoint shall hold the connection open with a ``:keepalive``
   comment on an idle interval, and shall replay a bounded ring of recent events
   on connect — filtered by the client's ``Last-Event-ID`` when present — before
   switching to the live broadcast, so a brief disconnect drops no events. The
   hand-off shall neither gap nor duplicate events.

.. req:: Health telemetry shape
   :id: REQ_0967
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0106, ADR_0125, TEST_0937

   The ``GET /api/v1/health`` document shall carry the golden's ``x-medkit-*``
   telemetry blocks (entity-cache, data-provider, subscription-executor),
   field-complete, plus a wall-clock ``timestamp``. The entity-cache counts
   shall be real; the provider/executor blocks are best-effort placeholders
   (benign zeros) until a richer provider lands, so a field-hardcoding client
   never hits a missing key.

.. req:: Auth disable parity
   :id: REQ_0968
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0112, ADR_0125, TEST_0939

   The ``/api/v1/auth/*`` endpoints shall be mountable or absent by
   configuration. When auth is disabled the three paths shall answer a
   contract-shaped ``404`` (the family is *absent*, matching an upstream
   ``ros2_medkit`` started with auth off), not the ``501`` deferred fallback.
   Enforcement of issued tokens (real JWT/RBAC) remains deferred to #87
   regardless of the flag (:need:`REQ_0938`).

Write plane — operations (simulation-backed)
--------------------------------------------

The first write family, built on a command-side seam that mirrors the read
``Provider`` seam. v1 is backed by an in-memory simulation that performs **no
real effect**, so it touches no safety-critical resource and the write-surface
safety gate (:need:`ADR_0119`) is not yet engaged; the gate re-enters at the
seam when a real-effect binding lands (:need:`ADR_0126`).

.. req:: Write/action seam
   :id: REQ_0969
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0121, ADR_0126, TEST_0943

   The gateway shall perform every write through an ``ActionSink`` trait — the
   command-side analogue of the read ``Provider`` seam — never touching taktora
   directly. The crate shall ship an in-memory ``SimActionSink`` (configurable
   per-resource operation catalogue, synchronously-completing executions that
   echo their args) so the write surface is fully testable with no runtime and
   no real effect. The seam shall be the single substitution point at which a
   future ``SafetyGate``-wrapped real binding drops in without any handler
   change, preserving the extractable core (:need:`REQ_0916`).

.. req:: Operations family with async executions
   :id: REQ_0970
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0121, ADR_0126, TEST_0944, TEST_0945

   The gateway shall serve the SOVD operations family on every entity kind:
   ``GET …/operations`` (catalogue), ``GET …/operations/{op}`` (detail),
   ``GET`` / ``POST …/operations/{op}/executions`` (list / start), and
   ``GET`` / ``PUT`` / ``DELETE …/operations/{op}/executions/{exec_id}`` (poll /
   update / cancel). A start shall return ``202`` with the execution; an unknown
   operation or execution shall return ``404``; a cancel shall return ``204`` and
   remove the execution. These paths shall be carved out from under the ``501``
   deferred fallback and advertised honestly in the root capabilities
   (:need:`REQ_0965`).

.. req:: Configurations family
   :id: REQ_0971
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0121, ADR_0126, TEST_0946

   The gateway shall serve the SOVD configurations family on every entity kind
   through the ``ActionSink`` seam: ``GET …/configurations`` (list),
   ``GET …/configurations/{config_id}`` (one; ``404`` if unset),
   ``PUT …/configurations/{config_id}`` (upsert → ``200`` with the stored entry),
   ``DELETE …/configurations/{config_id}`` (``204``; ``404`` if unset), and
   ``DELETE …/configurations`` (delete all → ``204``). The simulation stores
   values in memory and performs no real effect (:need:`ADR_0126`).

.. req:: Bulk-data family
   :id: REQ_0972
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0121, ADR_0126, TEST_0947

   The gateway shall serve the SOVD bulk-data family on apps and components:
   ``GET …/bulk-data`` (categories), ``GET …/bulk-data/{category}`` (descriptors),
   ``POST …/bulk-data/{category}`` (upload the raw body → ``201`` with a
   descriptor), ``GET …/bulk-data/{category}/{file_id}`` (download the stored
   bytes), and ``DELETE …/bulk-data/{category}/{file_id}`` (``204``). Unknown
   category/file is ``404``. The simulation stores bytes in memory (no multipart
   parsing, no real effect — :need:`ADR_0126`).

.. req:: Scripts family
   :id: REQ_0973
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0121, ADR_0126, TEST_0948

   The gateway shall serve the SOVD scripts family on apps and components: upload
   (``POST …/scripts`` → ``201``), list/get/delete script metadata, and an
   executions sub-resource reusing the operations execution model
   (``POST …/scripts/{id}/executions`` → ``202``;
   ``GET`` / ``PUT`` / ``DELETE …/executions/{exec_id}``). Unknown script or
   execution is ``404``. The simulation completes executions synchronously and
   performs no real effect (:need:`ADR_0126`).

.. req:: Software-update family
   :id: REQ_0974
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0121, ADR_0126, TEST_0949

   The gateway shall serve the SOVD software-update family as a **global**
   surface (not entity-scoped): ``GET`` / ``POST /updates`` (list / register →
   ``201``), ``GET /updates/{id}`` (detail), ``GET /updates/{id}/status``,
   ``PUT /updates/{id}/{prepare,execute,automated}`` (transition → ``202``), and
   ``DELETE /updates/{id}`` (``204``). Unknown id is ``404``. The simulation
   tracks status in memory (``registered`` → ``prepared`` → ``executed``) and
   performs no real effect (:need:`ADR_0126`).

.. req:: Lifecycle-status family
   :id: REQ_0975
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0121, ADR_0126, TEST_0950

   The gateway shall serve entity lifecycle transitions on apps and components:
   ``GET …/status`` (current state, default ``running``) and
   ``PUT …/status/{start,restart,shutdown,force-restart,force-shutdown}``
   (transition → ``202`` with the new state). ``start``/``restart``/
   ``force-restart`` map to ``running`` and ``shutdown``/``force-shutdown`` to
   ``stopped``; an unrecognised transition is ``400``. The simulation tracks
   state in memory and performs no real effect (:need:`ADR_0126`).

Read-family completion
----------------------

The remaining read thin spots: the two deferred read families (logs,
cyclic-subscriptions) and two best-effort served surfaces (health telemetry,
single-entity catalogue), brought to contract fidelity (:need:`ADR_0127`).

.. req:: Logs family
   :id: REQ_0976
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0122, ADR_0127, TEST_0951

   The gateway shall serve the SOVD logs family on every entity kind:
   ``GET …/logs`` with optional ``?severity=`` (exact) and ``?context=``
   (substring) filters returning the matching log entries, plus
   ``GET`` / ``PUT …/logs/configuration``. Log entries are sourced from the read
   ``Provider`` snapshot; the configuration is held through the ``ActionSink``
   write seam. The family is carved out of the ``501`` fallback and advertised
   honestly (:need:`REQ_0965`).

.. req:: Cyclic-subscriptions family
   :id: REQ_0977
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0122, ADR_0127, TEST_0952

   The gateway shall serve the SOVD cyclic-subscriptions family on apps,
   components, and functions: CRUD over a subscription (entity-scoped, pinned to
   the path entity, cross-entity access ``404``) plus a
   ``…/{sub_id}/events`` SSE stream that periodically samples the entity's data
   (at the subscription's interval) and pushes it in the golden SSE frame shape.
   Carved out of the ``501`` fallback and advertised honestly.

.. req:: Provider-sourced health telemetry
   :id: REQ_0978
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0122, ADR_0127, TEST_0953

   The ``GET /health`` document shall overlay provider-supplied telemetry over
   the default ``x-medkit-{data-provider,subscription-executor,entity-cache}``
   blocks, so a provider that reports real pool/executor counters surfaces them
   while one that does not yields the prior zero baseline (back-compatible). The
   live entity-cache counts shall remain authoritative and never be shadowed by
   an override (resolving the best-effort gap noted in :need:`REQ_0967`).

.. req:: Single-entity capability catalogue
   :id: REQ_0979
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0122, ADR_0127, TEST_0954

   The single-entity detail (``GET /{collection}/{id}``) shall carry, per kind,
   the golden's richer shape: a ``_links`` including the entity's relations, flat
   per-sub-resource href keys, and a ``capabilities`` array of
   ``{ name, href }`` for every sub-resource the entity exposes. The per-kind set
   shall match the captured ``*_get.json`` goldens for apps/components/functions
   and be derived from the mounted surface for areas, driven by a single per-kind
   relation/segment source so routes and links cannot drift.

Build identity
--------------

The read surface advertised only the crate semver, so a field issue could not be
tied back to the exact source a binary was built from. This pins the *build* —
the commit, whether the tree was clean, and when it was built — into the version
catalogue, captured at compile time and injected as data so the extractable core
stays dependency-clean (:need:`ADR_0128`).

.. req:: Build identity in the version catalogue
   :id: REQ_0980
   :status: implemented
   :satisfies: FEAT_0100
   :links: BB_0123, ADR_0128, TEST_0955

   The ``GET /api/v1/version-info`` document shall report, under ``vendor_info``,
   the identity of the source the binary was built from: the full and short git
   commit hash, a working-tree dirty flag, a ``git describe`` string (nearest tag
   plus distance, or the short hash when untagged), the build timestamp (UTC,
   RFC3339), and the ``rustc`` version — alongside the existing crate ``version``.
   The identity shall be captured at build time and travel with the binary, so a
   deployed device reports its exact commit with no runtime configuration. When
   git metadata is unavailable at build time — for example a build from a
   published crates.io tarball, which carries no ``.git`` — the git-derived
   fields shall degrade to ``"unknown"`` and the build shall not fail. The added
   fields shall be additive under ``vendor_info`` so a client written against the
   ros2_medkit contract (:need:`REQ_0911`) reads the document unchanged.

Requirements at a glance
------------------------

.. needtable::
   :columns: id, title, status, satisfies
   :show_filters:
   :filter: "FEAT_0100" in satisfies
