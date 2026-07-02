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

.. arch-decision:: Zero-alloc test enforcement scoped to executor + connector pre-1.0
   :id: ADR_0133
   :status: accepted
   :refines: ADR_0114
   :links: REQ_0925, TEST_0914

   **Context.** Allocation-freedom is asserted across the workspace by
   counting-global-allocator differential tests (the ``TEST_0194`` pattern:
   ``count(big) − count(small)`` under a process-wide counter). That harness
   counts *every* thread's allocations during the tracking window, so it is
   inherently sensitive to platform-allocator and harness noise — the class of
   intermittent off-by-one failures documented in GitHub #132. Each such test
   is a flake liability, and the medkit diagnostics surface had inherited one
   (:need:`TEST_0914` for :need:`REQ_0925`) even though medkit is
   pre-1.0 diagnostics tooling, not certified control-path code.

   **Decision.** Pre-1.0, *verified* zero-allocation (a counting-allocator
   regression test in CI) is required only where the property is load-bearing:
   **executor scope** (the dispatch / telemetry fold on the ``WaitSet``
   thread, :need:`REQ_0104`) and **connector scope** (the cyclic channel
   registry path, :need:`REQ_0328`). Everywhere else — medkit first —
   allocation-freedom may remain a *design property* (ADR_0114's per-task
   atomic sink allocates nothing by construction) without a test enforcing
   it: :need:`REQ_0925` is softened accordingly and :need:`TEST_0914` is
   retired. Existing no-alloc guards outside the two mandated scopes (motion,
   DLT logging) may stay while they are quiet, but they are regression
   guards, not requirements, and are dropped rather than stabilised if they
   flake.

   **Alternatives considered.**

   * *Harden the harness instead* (thread-scoped counting, retry-on-flake,
     platform skips). Rejected: real engineering cost to keep a guarantee
     medkit does not need before 1.0.0, and retries would normalise a
     flaky-by-design gate.
   * *Keep the mandate but mark the test* ``#[ignore]`` *on noisy platforms.*
     Rejected: leaves the requirement claiming a verification that no longer
     runs everywhere — a silent traceability lie.

   **Consequences.** ✅ The flake class leaves the medkit suite; CI
   signal-to-noise improves without touching the binding's design. ✅ The
   enforcement boundary is now explicit and citable for future subsystems
   (new no-alloc tests need an executor- or connector-scope justification).
   ❌ An allocation regression introduced into the medkit hook write path is
   no longer caught by CI; re-verifying :need:`REQ_0925` (with a
   thread-scoped harness) is deferred to the 1.0 hardening pass.

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

.. arch-decision:: Diff-derived event vocabulary over the golden frame shape
   :id: ADR_0117
   :status: accepted
   :refines: FEAT_0100

   **Context.** The captured fault-stream golden
   (``contract/golden/faults_stream_sse_sample.txt`` /
   ``faults_stream_event.json``) frames each Server-Sent Event as
   ``id: <n>`` / ``event: <event_type>`` / ``data: <json>`` and the ``data``
   object carries ``event_type``, a full ``fault`` sub-object, a ``timestamp``,
   and an ``x-medkit`` (``entity_id``, ``entity_type``); the captured
   ``event_type`` value is ``fault_confirmed``. taktora, however, has no
   confirmation signal to replay — its events come from **diffing successive
   merged views** (:need:`REQ_0930`), which yields a different, richer
   vocabulary (``fault_raised``, ``fault_cleared``, ``health_changed``). The two
   cannot both be authoritative.

   **Decision.** Split authority by layer. The **frame envelope and the
   data-object shape** are authoritative from the golden and are reproduced
   byte-for-byte (the ``FaultEvent`` DTO serializes to exactly those keys; the
   SSE frame writes ``id`` then ``event`` then a single-line ``data`` in the
   golden's field order). The **``event_type`` vocabulary** is taktora's
   diff-derived set; we deliberately do not emit ``fault_confirmed``. A
   ``health_changed`` event — which has no single originating fault — carries a
   representative ``fault`` (the worst current fault, or the just-cleared fault
   when health returns to ``OK``) so the uniform golden shape always holds. The
   ``contract/`` corpus is captured upstream data and stays read-only.

   **Consequences.** ✅ A drop-in ``ros2_medkit`` SSE client parses the stream
   unchanged — it reads ``event``/``data`` fields it already understands. ✅ The
   richer taktora vocabulary is available to taktora-aware clients via the
   ``event_type`` field without a second wire shape. ❌ A client that hard-codes
   the literal string ``fault_confirmed`` sees taktora's labels instead; this is
   documented as the intended reconciliation, not a regression. See
   :need:`REQ_0931`, :need:`REQ_0934`.

.. arch-decision:: Auth-light seam before real JWT/RBAC enforcement
   :id: ADR_0118
   :status: accepted
   :refines: FEAT_0100

   **Context.** A drop-in SOVD client authenticates via ``OAuth2``
   ``client_credentials`` → JWT ``Bearer`` *before* it reads any diagnostics; if
   ``/api/v1/auth/*`` does not exist its login fails and it never reaches the
   read surface it is compatible with. But full JWT validation + RBAC
   (``viewer`` / ``operator`` / ``configurator`` / ``admin``) and the enforcement
   modes (``none`` / ``write`` / ``all``) are out of scope for v1: the deployment
   posture is network-layer auth (NetBird / mTLS), with the gateway auth-light.
   The token-endpoint path was contested in the upstream docs; it is pinned from
   the running binary's OpenAPI to ``POST /api/v1/auth/token`` (singular),
   ``/auth/authorize``, ``/auth/revoke`` (POST-only). There is no captured golden
   token body — the demo image ran with auth disabled — so the response *shape*
   is derived from the OpenAPI ``getToken`` / ``AuthTokenResponse`` schema plus
   the acceptance fields.

   **Decision.** Ship an auth-light v1 that preserves the client login flow
   behind a seam. Introduce an ``Authenticator`` trait (:need:`BB_0112`) as the
   substitution point; the default ``PermissiveAuthenticator`` is dev-mode (any
   credentials succeed, any/no ``Bearer`` token accepted) and issues a
   **shape-valid, unsigned** JWT — hand-rolled
   ``base64url(header).base64url(payload).signature`` with an ``alg: "none"``
   header — rather than pulling in a JWT/crypto dependency for a token v1 only
   needs to *shape*-validate. Resource routes run enforcement = none: a ``Bearer``
   token is accepted and never verified, so requests with or without one always
   pass. The authenticator is injected at router assembly
   (``router_with_authenticator``), not referenced by any handler, so a strict
   impl substitutes without reworking handlers. Real signing/validation, RBAC,
   and the enforcement modes are deferred to tracking issue #87 behind this seam.
   See :need:`REQ_0935`, :need:`REQ_0936`, :need:`REQ_0937`, :need:`REQ_0938`,
   :need:`REQ_0939`.

   **Consequences.** ✅ A ``client_credentials`` client completes login and reads
   the surface today, with no crypto dependency added. ✅ The trait seam and
   router-level injection mean the #87 strict path lands without touching read
   handlers. ✅ The enforcement = none posture matches the network-layer-auth
   deployment without prematurely committing to an RBAC model. ❌ The issued token
   is not cryptographically real and must not be trusted as an authorization
   decision until #87; ❌ a second seam (verification) is carried but advisory in
   v1, so a rejecting authenticator changes token issuance but not resource
   access while enforcement is none.

.. arch-decision:: Diagnostic write surface gated by Freedom-From-Interference; only QM-scoped families are v1
   :id: ADR_0119
   :status: accepted
   :refines: FEAT_0100

   **Context.** medkit is a QM-grade, off-control-path diagnostic surface
   (:need:`REQ_0910`, :need:`ADR_0111`). Adding SOVD write/action families would
   let a diagnostic client cause an effect on a live real-time control system for
   the first time.

   **Decision.** A QM→SC write is **forbidden** by the safety argument:
   :need:`AFSR_0002` (a reader of integrity ``L_r`` may only receive from a writer
   of ``L_w >= L_r``) and :need:`TSR_0007` (the SC process holds the *only* write
   capability to its shared memory); :need:`ADR_0050` rejected cohosting for the
   same reason. medkit's off-control-path placement (:need:`ADR_0111`) is
   therefore a load-bearing safety assumption, not a convenience. A diagnostic
   write into SC state instantiates :need:`AHZ_0001` (cycle starvation / halt) and
   :need:`AHZ_0002` (erroneous output) — the hazards :need:`ASG_0002` exists to
   prevent. **Consequence:** of the six write families, only those that touch
   **no** SC resource ship in v1 (locks — see :need:`ADR_0120`); every
   SC-affecting family (operations / executions #150, configurations-write #151,
   bulk-data #152, scripts #153, OTA #154) is gated behind: a HARA update, an
   SC-rated (ASIL B(D)) command-acceptance gate mediating QM→SC with a safe-state
   precondition, FTTI-compatible detection (FTTI = 100 ms per :need:`AOU_0006`;
   internal-fault detection within FTTI/2 = 50 ms per :need:`AFSR_0004`), watchdog
   interaction (:need:`AOU_0016`), and a strict ``Authenticator`` replacing the
   permissive :need:`ADR_0118` default (authz + rate-limit + audit). OTA
   additionally needs its own item-level safety case (replacing the running SC
   binary terminates the safety function).

   **Alternatives considered.** (a) Ship all write families with gateway-side
   guards only — rejected, violates :need:`AFSR_0002`. (b) Cohost a write path in
   the SC process now — rejected per :need:`ADR_0050`. (c) Defer the entire write
   surface indefinitely — rejected; locks is safe and unblocks future coordinated
   writes (issue #149).

   **Consequences.** ✅ The QM→SC boundary stays a hard safety invariant rather
   than a per-feature judgement call. ✅ locks (:need:`ADR_0120`) ships now as the
   one zero-SC-coupling family. ❌ Every other write family carries a HARA + SC-gate
   prerequisite before it can land.

.. arch-decision:: Locks are diagnostic-coordination-only QM metadata
   :id: ADR_0120
   :status: accepted
   :refines: FEAT_0100

   **Context.** Of the six SOVD write families, locks is the only one that can be
   built clean-room in v1 without a HARA update (:need:`ADR_0119`): a lock
   coordinates diagnostic clients against each other and governs no
   safety-critical resource.

   **Decision.** The lock registry (:need:`BB_0113`, issue #149) is **in-memory,
   off the control path, and guards nothing SC**. It is pure QM coordination
   metadata: at most one live lock per ``(entity-kind, id)`` resource, TTL-expiry
   auto-release against an injectable clock, ``break_lock`` supervisor override,
   and ``X-Client-Id`` ownership. It adds **no** edge to the executor / connector
   binding crates and **no** taktora-runtime dependency, so the extractable-core
   invariant (:need:`ADR_0111`, :need:`REQ_0916`) holds and the surface stays
   strictly QM — out of any HARA update. **The moment a lock guards an SC
   resource, the full write-surface gate of :need:`ADR_0119` applies**: it would
   become a QM→SC mediator and inherit every prerequisite recorded there.

   **Alternatives considered.** (a) Back locks with an SC-managed resource handle
   so they actually arbitrate control access — rejected; that is exactly the
   QM→SC write :need:`ADR_0119` forbids without the full gate. (b) Persist locks
   across restarts — deferred; in-memory is sufficient for diagnostic-session
   coordination and adds no durability surface.

   **Consequences.** ✅ locks ships as a strictly-QM v1 feature with no HARA
   impact. ✅ The extractable core is preserved. ❌ Locks provide no guarantee
   against a non-diagnostic actor (e.g. the SC process itself) — they coordinate
   diagnostic clients only, by design.

.. arch-decision:: Tier-A wire-compatibility parity pass
   :id: ADR_0125
   :status: accepted
   :links: REQ_0961, REQ_0962, REQ_0963, REQ_0964, REQ_0965, REQ_0966, REQ_0967, REQ_0968

   **Context.** A gap analysis of the served surface against the captured
   contract found breaks *inside* families already nominally implemented: the
   global ``/faults/stream`` was absent (the diff stream lived only at
   ``/triggers/events``); triggers were mounted only globally, not per entity as
   the contract paths require; locks exposed no ``GET``; there was no global
   ``DELETE /faults``; the root capability flags advertised served extensions as
   ``false``; the SSE stream had no keep-alive or reconnect replay; the health
   document omitted the golden ``x-medkit-*`` telemetry blocks; and auth was
   always mounted, so it could not match an upstream started with auth off. Each
   would surface a drop-in ``ros2_medkit`` client to a 404/parse-error/skip even
   though the family was "done".

   **Decision.** Land these as one cohesive parity pass (:need:`REQ_0961` –
   :need:`REQ_0968`), all in ``gateway-axum`` plus the ``view.rs`` root/health
   documents, touching no binding crate. The pass is strictly QM and adds **no**
   write to a safety-critical resource — the global clear-all is a shape-only
   ``204`` acknowledgement like the per-entity fault ``DELETE`` — so
   :need:`ADR_0119` is untouched and no HARA update is triggered. The honest
   capability advertisement (:need:`REQ_0965`) makes the served-vs-deferred
   boundary machine-readable at the root.

   **Alternatives.** (a) Treat the trigger surface as "good enough" globally —
   rejected; a path-hardcoding client reaches triggers only at
   ``/{collection}/{id}/triggers``. (b) Repoint the trigger stream rather than
   add ``/faults/stream`` — rejected; the contract has *both*, with distinct
   semantics (global vs trigger-filtered).

   **Consequences.** ✅ A read/coordination ``ros2_medkit`` client (dashboards,
   monitors, fault viewers) is now drop-in. ✅ No safety entanglement; the
   ``501`` write families remain the documented boundary. ❌ Full wire
   compatibility still excludes the write/action families gated by
   :need:`ADR_0119`. ❌ The health provider/executor telemetry stays best-effort
   (zeros) until a richer provider lands (:need:`REQ_0967`).

.. arch-decision:: Write plane as a port/adapter seam with a deferred safety gate
   :id: ADR_0126
   :status: accepted
   :links: REQ_0969, REQ_0970

   **Context.** The write/action families (operations, configurations,
   bulk-data, scripts, updates, lifecycle) were ``501`` by design under
   :need:`ADR_0119`, which forbids a QM→SC write from the diagnostic plane
   without a per-family safety case. We want the write *surface* — wire-shape,
   async-execution model, testability — buildable now without waiting on those
   safety cases or on deep taktora runtime integration.

   **Decision.** Model the write side as a ports-&-adapters seam mirroring the
   read ``Provider`` seam: an ``ActionSink`` trait (the facade) the gateway
   depends on, an in-memory ``SimActionSink`` adapter that **performs no real
   effect** (executions complete synchronously in memory), and the HTTP handlers
   as thin adapters over the trait. Because the only adapter is the simulation,
   **no safety-critical resource is touched**, so :need:`ADR_0119` is not yet
   engaged — exactly as Locks shipped (real surface, guards nothing). The safety
   gate re-enters as a ``SafetyGate<RealBindingSink>`` decorator **at this seam**
   when a real-effect binding lands with deep taktora support; no handler
   changes.

   **Alternatives.** (a) Wait for the per-family safety cases before any write
   code — rejected; it blocks the entire surface and its client integration on
   work that is months out. (b) Implement effects now behind a runtime check —
   rejected; that *is* the QM→SC write :need:`ADR_0119` forbids without the full
   gate.

   **Consequences.** ✅ The write surface is shape-complete, wire-compatible, and
   fully testable against the simulation today. ✅ The safety boundary is
   preserved — the only backend performs no effect, and the gate has a defined
   insertion point. ❌ A client cannot yet cause a real effect (by design). ❌
   The advertised ``operations: true`` capability now means "the surface exists",
   not "effects occur" — documented here and in :need:`REQ_0969`.

.. arch-decision:: Read-family completion — seam choices
   :id: ADR_0127
   :status: accepted
   :links: REQ_0976, REQ_0977, REQ_0978, REQ_0979

   **Context.** After the read core, Tier-A parity, and the write plane, four
   read thin spots remained: the deferred ``logs`` and ``cyclic-subscriptions``
   families, the best-effort (zero-filled) ``/health`` telemetry, and the thin
   single-entity detail (missing the golden's ``capabilities`` catalogue). Each
   needs a data source, and the question is which existing seam to reuse.

   **Decision.** Reuse the established seams rather than invent new ones:
   **logs** entries flow through the read ``Provider`` snapshot (a ``logs`` map,
   folded like ``data``) while the small logs **configuration** read/write rides
   the ``ActionSink`` write seam — splitting read data from write-state along the
   existing read/write seam boundary. **Cyclic-subscriptions** is a self-contained
   per-connection periodic sampler (an interval stream over the read view),
   simpler than the triggers broadcast/diff loop because it pushes current data,
   not diffs. **Health telemetry** is an additive ``Telemetry`` overlay on the
   snapshot: the gateway overlays provider-supplied keys over the default blocks
   and keeps the live entity-cache counts authoritative, so absent telemetry is
   byte-for-byte back-compatible. The **single-entity catalogue** is built from a
   single per-kind relation/segment source shared with the router, so the
   advertised links cannot drift from the mounted routes.

   **Consequences.** ✅ The SOVD read surface is contract-complete (only the
   genuinely-out-of-scope families remain ``501``). ✅ No new seam types; the
   read/write boundary stays clean. ❌ Cyclic sampling captures the view snapshot
   at connect (live-refresh is a future refinement). ❌ Telemetry/log values are
   still simulation-sourced until a real binding lands (:need:`ADR_0126`).

.. arch-decision:: Compile-time build identity, captured in a leaf crate and injected as data
   :id: ADR_0132
   :status: accepted
   :links: REQ_0990

   **Context.** The version catalogue reported only the crate semver
   (``CARGO_PKG_VERSION`` baked at compile time). A field issue could not be tied
   back to the exact source a binary was built from — there was no commit, no
   dirty state, no build time on the wire. Two questions: where the identity
   comes from, and how it reaches the surface without breaching the
   extractable-core invariant (:need:`REQ_0916`, :need:`ADR_0111`), under which
   the ``model`` / ``provider`` / ``gateway`` / ``gateway-axum`` crates carry no
   edge out to a non-medkit ``taktora-*`` crate.

   **Decision.** Capture at **compile time**, not runtime. A leaf
   ``taktora-build-info`` crate whose hand-rolled ``build.rs`` (zero
   dependencies) shells ``git rev-parse`` / ``git describe`` /
   ``git status --porcelain`` and records the UTC build timestamp and ``rustc``
   version, emitting them as ``cargo:rustc-env`` constants its ``lib.rs`` reads
   back into a ``BuildInfo``. Identity travels *with* the binary, so a deployed
   device names its commit with no runtime configuration and no way to lie by
   misconfiguration. A failing git command degrades that field to ``"unknown"``
   rather than failing the build, so a build from a published crates.io tarball
   (no ``.git``) still compiles. Reach the surface by **injection, not
   dependency**: the ``BuildInfo`` DTO lives in ``taktora-medkit-model``
   (defaulting to all-``"unknown"``), ``version_info_document`` renders a
   ``&BuildInfo`` under ``vendor_info``, and the application binary — outside the
   extractable core — calls ``taktora_build_info::capture()`` and wires it into
   the ``gateway-axum`` router builder. Build identity thus flows through the
   same injected-seam pattern as ``Provider`` / ``ActionSink`` / ``Manifest``.

   **Consequences.** ✅ A running binary reports its exact commit, dirty state,
   and build time; a field issue traces to source. ✅ The extractable core keeps
   zero edge to ``taktora-build-info`` (only binaries and examples depend on it),
   so the diagnostics folder still lifts out cleanly, and the capture crate is
   reusable by any taktora binary, not just medkit. ✅ The fields are additive
   under ``vendor_info``, so a ``ros2_medkit`` client reads ``/version-info``
   unchanged (:need:`REQ_0911`). ❌ The ``git_dirty`` flag reflects the tree at
   the last ``build-info`` recompile, not necessarily every rebuild; CI and
   release artefacts are clean rebuilds, so it is accurate on the binaries that
   actually ship. ❌ A binary that forgets to inject reports an honest
   ``"unknown"`` rather than failing loudly — the default is safe, not noisy.

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

.. building-block:: triggers + SSE event stream
   :id: BB_0111
   :status: implemented
   :implements: REQ_0930, REQ_0931, REQ_0932, REQ_0933, REQ_0934
   :links: TEST_0919, TEST_0920, TEST_0921

   The live-push slice inside ``taktora-medkit-gateway-axum`` (:need:`BB_0107`),
   off the request path on the tokio side. A **refresh-and-diff loop** re-polls
   and re-merges the provider snapshot on a cadence, hot-swaps the served
   ``MergedView`` through a ``watch`` channel (so the read-core handlers see the
   live view via a ``FromRef`` extraction and are oblivious to the swap), diffs
   successive views, and broadcasts the change events
   (``fault_raised`` / ``fault_cleared`` / ``health_changed``) over a
   ``tokio::sync::broadcast``. A **trigger registry** behind the
   ``/api/v1/triggers`` CRUD routes holds basic entity/severity subscriptions;
   ``GET /api/v1/triggers/events`` subscribes to the broadcast, filters by the
   registered triggers, and renders each event as an SSE frame in the captured
   golden shape (:need:`ADR_0117`). Rich condition predicates are deferred to
   issue #87. No taktora-runtime edge is added — the loop reads only through the
   ``Provider`` seam, so the crate stays part of the extractable core.

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

.. building-block:: Authenticator seam (auth-light)
   :id: BB_0112
   :status: implemented
   :implements: REQ_0935, REQ_0936, REQ_0937, REQ_0938, REQ_0939
   :links: ADR_0118, TEST_0922, TEST_0923, TEST_0924

   The gateway's authentication seam, inside ``taktora-medkit-gateway-axum``
   (:need:`BB_0107`). An ``Authenticator`` trait carries token issuance
   (``issue_token``) and bearer verification (``verify_bearer``) behind one
   substitution point; the default ``PermissiveAuthenticator`` is dev-mode and
   issues a shape-valid, unsigned JWT (hand-rolled
   ``base64url(header).base64url(payload).signature``, no crypto dependency). A
   small ``auth`` sub-router mounts the POST-only ``/api/v1/auth/token`` /
   ``authorize`` / ``revoke`` endpoints over the trait, carved out from under the
   deferred-family ``501`` fallback, and is merged into the main router; the
   authenticator is injected at assembly (``router_with_authenticator``) so it is
   absent from every read-core handler. Resource routes run enforcement = none.
   The seam is the drop-in point for the deferred strict path — real JWT
   validation, RBAC, and enforcement modes (tracking issue #87) — which lands
   without reworking handlers (:need:`ADR_0118`).

.. building-block:: diagnostic lock registry
   :id: BB_0113
   :status: implemented
   :implements: REQ_0940, REQ_0941, REQ_0942, REQ_0943, REQ_0944
   :links: ADR_0119, ADR_0120, TEST_0925, TEST_0926, TEST_0927

   The lock-registry slice inside ``taktora-medkit-gateway-axum``
   (:need:`BB_0107`), off the request path. An in-memory ``LockRegistry`` — a
   ``HashMap`` keyed by ``(entity-kind, id)`` behind a ``Mutex`` — backs the
   ``POST`` / ``PUT`` / ``DELETE`` routes under
   ``/api/v1/{apps,components}/{id}/locks`` (the two entity kinds the contract
   exposes ``/locks`` on), carved out from under the deferred-family ``501``
   fallback. Acquire returns a contract-shaped ``Lock`` with an absolute RFC3339
   ``lock_expiration`` (a millisecond TTL in the request); TTL expiry
   auto-releases against an **injectable** ``Clock`` so the behaviour is
   deterministic in tests; ``break_lock`` evicts a held lock (supervisor
   override); ownership is enforced by the ``X-Client-Id`` holder header, and a
   second client without ``break_lock`` gets ``409``. The registry guards no
   safety-critical resource and adds no taktora-runtime edge, so the crate stays
   part of the extractable core (:need:`ADR_0120`). One minimal, zero-dependency
   ``humantime`` dependency formats the RFC3339 instant. See :need:`ADR_0119`.

.. building-block:: SSE replay ring + keep-alive
   :id: BB_0120
   :status: open
   :implements: REQ_0961, REQ_0966

   A bounded (100-event) replay ring retained alongside the change-event
   broadcast in ``gateway-axum``'s ``triggers`` module. A shared SSE builder
   subscribes the live broadcast, snapshots the ring (filtered by
   ``Last-Event-ID`` and the endpoint's pass predicate), replays it, then chains
   the live stream — dropping anything already replayed so the hand-off neither
   gaps nor duplicates — and attaches a ``:keepalive`` comment interval. The
   global ``/faults/stream`` (pass = all), ``/triggers/events`` (pass = any
   registered trigger), and per-trigger ``…/events`` (pass = that trigger) all
   build on it. Off the control path; no taktora-runtime edge.

.. building-block:: ActionSink write seam + operations surface
   :id: BB_0121
   :status: open
   :implements: REQ_0969, REQ_0970, REQ_0971, REQ_0972, REQ_0973, REQ_0974, REQ_0975

   The command-side seam, mirroring the read ``Provider`` seam. In
   ``taktora-medkit-provider``: the ``ActionSink`` trait (operations catalogue,
   start/list/get/cancel executions), the wire types (``ResourceRef``,
   ``OperationDef``, ``Execution``, ``ExecutionStatus``, ``ActionError``), and the
   in-memory ``SimActionSink`` (per-resource catalogue, synchronously-completing
   echo executions) — zero taktora dependencies, so it stays in the extractable
   core. In ``taktora-medkit-gateway-axum``: ``actions.rs`` mounts the per-kind
   ``operation_routes`` as thin adapters over the trait, and ``ServerState``
   carries an ``Arc<dyn ActionSink>`` (defaulting to an empty ``SimActionSink``,
   injectable via ``router_with_actions`` / ``serve_listener_with_actions``). The
   safety gate decorator slots in at the trait when a real binding lands
   (:need:`ADR_0126`).

.. building-block:: Read-family completion
   :id: BB_0122
   :status: open
   :implements: REQ_0976, REQ_0977, REQ_0978, REQ_0979

   The surfaces that bring the read side to contract fidelity (:need:`ADR_0127`).
   In ``taktora-medkit-provider``: a ``logs`` map and a ``Telemetry`` overlay on
   ``ProviderSnapshot`` (plus ``MockProvider`` builders) and the logs-configuration
   methods on ``ActionSink``/``SimActionSink``. In ``taktora-medkit-gateway``: the
   ``MergedView::logs`` resolver and the ``health_document`` telemetry overlay. In
   ``taktora-medkit-gateway-axum``: ``logs.rs`` (log routes), ``cyclic.rs``
   (subscription registry + periodic-sampling SSE, reusing the keep-alive
   infrastructure), and the enriched ``entity_detail`` catalogue driven by a
   per-kind relation/segment source shared with the router. Off the control path;
   no taktora-runtime edge.

.. building-block:: Build identity — capture crate + injection seam
   :id: BB_0123
   :status: open
   :implements: REQ_0990

   A leaf ``publish = false`` crate ``taktora-build-info`` plus the seam that
   carries its output into the read surface without an edge into the extractable
   core (:need:`ADR_0132`). ``taktora-build-info``'s hand-rolled ``build.rs``
   (zero dependencies) shells ``git rev-parse HEAD`` / ``--short HEAD``,
   ``git describe --tags --always``, and ``git status --porcelain`` (→ dirty
   flag), records the UTC build timestamp and ``rustc --version``, and emits each
   as a ``cargo:rustc-env=TAKTORA_BUILD_*`` value, with ``cargo:rerun-if-changed``
   on ``.git/HEAD`` and the resolved ref so a rebuild after a new commit
   re-captures the hash instead of keeping a stale one; any failing git command
   yields ``"unknown"``. Its ``lib.rs`` reads the values back through ``env!``
   into a ``capture() -> BuildInfo``. The ``BuildInfo`` DTO lives in
   ``taktora-medkit-model`` (all-``"unknown"`` default), so the core owns the
   shape; ``taktora-medkit-gateway``'s ``version_info_document`` takes a
   ``&BuildInfo`` and renders it under ``vendor_info``; and
   ``taktora-medkit-gateway-axum`` threads it through the router builder
   (``with_build_info``), defaulting to the unknown ``BuildInfo``. Only the
   application binary and examples depend on ``taktora-build-info`` and call
   ``capture()`` to inject — the core keeps zero edge to it, holding the
   extractable-core invariant (:need:`REQ_0916`).

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
