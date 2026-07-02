Runtime diagnostics (medkit) tests
==================================

Test cases verifying the :need:`FEAT_0100` requirement cluster. Each ``test``
``:verifies:`` one or more ``req`` parents. The grounding slice establishes the
``TEST_0900`` IDs and the scenarios; downstream slices flip the requirements
they verify to ``implemented`` and link these tests.

.. test:: Model wire shapes round-trip
   :id: TEST_0900
   :status: open
   :verifies: REQ_0914, REQ_0915

   Each model DTO (entity, DTC/fault with status sub-object and freeze-frame,
   collection envelope) survives a serialise → deserialise round-trip equal to
   the original. Establishes the wire-type surface the contract snapshot
   (:need:`TEST_0905`) later pins byte-for-byte.

.. test:: Core crates carry no taktora dependency
   :id: TEST_0901
   :status: open
   :verifies: REQ_0916

   ``cargo tree`` on each core crate (``-model``, ``-provider``, ``-gateway``,
   ``-gateway-axum``) shows no ``taktora-*`` edge, holding the extractable-core
   invariant of :need:`ADR_0111`.

.. test:: Gateway read core over the mock provider
   :id: TEST_0902
   :status: open
   :verifies: REQ_0912

   The transport-neutral gateway resolves the entity tree, fault lists, and the
   worst-wins health rollup over the mock ``Provider`` without any HTTP layer.

.. test:: Worst-wins health rollup
   :id: TEST_0903
   :status: open
   :verifies: REQ_0912

   An entity's aggregated health equals the worst health of itself and its
   descendants; faulting a leaf rolls its ancestors, and an ancestor returns to
   healthy only once its last faulting descendant clears.

.. test:: Off-path forwarding never blocks the control path
   :id: TEST_0904
   :status: open
   :verifies: REQ_0910, REQ_0913

   A binding's callback hook hands work to the bounded forwarding channel and
   returns without blocking or allocating; with the channel full, the hook
   drops rather than blocks, and the control-path caller is never stalled by a
   slow diagnostics consumer.

.. test:: Drop-in contract snapshot
   :id: TEST_0905
   :status: open
   :verifies: REQ_0911

   Serialising each served shape produces JSON whose keys and casing match the
   captured ros2_medkit contract corpus fixture, making drop-in compatibility a
   snapshot-tested regression guard.

.. test:: Read-core over a live server matches the contract shapes
   :id: TEST_0906
   :status: implemented
   :verifies: REQ_0917

   A live axum server over the mock provider is driven over real TCP; each
   read-core response (entity lists, single-entity views, relationship
   sub-resources, global and entity-scoped fault lists, the single-fault detail,
   data reads, and the not-found error) shape-matches its
   ``contract/golden/*.json`` fixture — every key, nesting, and value type the
   contract constrains is present, while values may differ since the live
   skeleton is one self-consistent snapshot rather than a byte replay of the
   mutually-inconsistent capture (byte fidelity is pinned separately by
   :need:`TEST_0905`).

.. test:: Deferred families return a contract-shaped 501
   :id: TEST_0907
   :status: implemented
   :verifies: REQ_0918

   A smoke test hits at least one route per deferred family on the live server
   and asserts ``501`` (not ``404``) with a ``GenericError`` body whose
   ``error_code`` is ``not-implemented`` and whose parameters name the family.

.. test:: Transport hardening is present and configurable
   :id: TEST_0908
   :status: implemented
   :verifies: REQ_0919

   The default server advertises a CORS allow-origin header, and a server
   configured with a one-token bucket throttles the second request to ``429``,
   demonstrating CORS and the rate limit are wired and configurable.

.. test:: Executor binding satisfies the provider seam
   :id: TEST_0912
   :status: implemented
   :verifies: REQ_0924

   A freshly constructed binding exposes the synthetic executor entity plus one
   App entity per registered task through the ``Provider`` seam, all reading
   healthy with no faults, so the gateway can read live executor state through
   the same seam it reads the mock through.

.. test:: A running executor drives binding liveness and timing
   :id: TEST_0913
   :status: implemented
   :verifies: REQ_0923, REQ_0924, REQ_0925

   A real ``taktora-executor`` with the binding registered as observer and
   monitor runs cyclic App items: the App entity health reflects the lifecycle
   hooks (a running App reads healthy, an erroring App degrades to ``Error``),
   and the readable ``data`` timing (execution count, EWMA, period / rate)
   updates from ``post_execute`` and ``on_cycle_stats``. Because the hooks
   fire on the live executor's ``WaitSet`` thread while the provider reads
   off-path, this test also stands as the verification of the bounded,
   non-blocking hook write path (:need:`REQ_0925`) after the retirement of
   the counting-allocator test (:need:`TEST_0914`, :need:`ADR_0133`).

.. test:: Hook write path performs zero heap allocation
   :id: TEST_0914
   :status: rejected
   :verifies: REQ_0925

   The binding's hooks are driven in a steady-state loop under a counting global
   allocator; a differential measurement (``count(big) − count(small)``) shows
   zero allocations per hook cycle, with a deliberate-allocation negative case
   proving the counter still observes this thread — mirroring the executor's own
   cycle-stats allocation test and holding :need:`ADR_0111`.

   **Retired.** Zero-alloc test enforcement is scoped to executor and
   connector scope pre-1.0 (:need:`ADR_0133`); the counting-global-allocator
   harness is flake-prone (GitHub #132) and :need:`REQ_0925` no longer
   mandates verified allocation-freedom. The test file
   (``hook_no_alloc.rs``) is removed; the property stays true by
   construction per :need:`ADR_0114`.

.. test:: Builder and TOML manifests agree
   :id: TEST_0909
   :status: implemented
   :verifies: REQ_0920

   The same skeleton built via ``Manifest::builder`` and parsed via
   ``Manifest::from_toml_str`` compares equal, and the committed example
   ``medkit.toml`` parses into a non-empty manifest whose ``parent_of`` resolves
   the ``app:<task>`` / ``component:<subdevice>`` id conventions.

.. test:: Manifest re-parents raw entities over a live server
   :id: TEST_0910
   :status: implemented
   :verifies: REQ_0920, REQ_0921

   A live axum server whose ``GatewayConfig`` carries a manifest, backed by a
   provider emitting only flat raw entities, returns the declared component under
   ``GET /api/v1/areas/{id}/components`` and the re-parented apps / subdevices
   under the component ``…/hosts`` and ``…/subcomponents`` sub-resources.

.. test:: Empty manifest yields flat grouping
   :id: TEST_0911
   :status: implemented
   :verifies: REQ_0922

   Folding with an empty (default) manifest injects no skeleton and leaves the
   raw entities parentless — the same view a no-manifest fold produces — and a
   live server with no manifest serves empty Areas and empty component nesting
   sub-resources without panicking.

.. test:: Connector health surfaces a Component and DTCs
   :id: TEST_0915
   :status: implemented
   :verifies: REQ_0926

   A simulated ``Up → Degraded → Down → Up`` health sequence fed into the
   connector binding, read back through the running gateway, presents one raw
   SOVD Component whose worst-wins health tracks the connector state (Ok →
   Warning → Critical → Ok) and whose fault list carries the Warning
   ``FIELDBUS_DEGRADED`` and Critical ``FIELDBUS_NOT_OPERATIONAL`` DTCs with the
   degraded reason string. A live axum server over the binding serves the same
   Component, fault list, and camelCase DTC status sub-object over real TCP.

.. test:: DTC lifecycle and occurrence bookkeeping
   :id: TEST_0916
   :status: implemented
   :verifies: REQ_0927

   Repeated degraded episodes, each cleared by a return to ``Up``, increment the
   DTC occurrence count and widen the first/last occurrence window; a healed DTC
   reports ``testFailed`` cleared with ``confirmedDTC`` still latched and remains
   in memory, and the Component rolls back to healthy only once the last active
   DTC clears.

.. test:: Confirmed DTC carries a last-sample freeze-frame
   :id: TEST_0917
   :status: implemented
   :verifies: REQ_0928

   A confirmed DTC carries a freeze-frame under the contract's ``snapshots`` /
   ``extended_data_records`` shape whose payload is the last connector hook
   sample observed before confirmation (or a synthesized health snapshot when no
   sample was observed), reachable through the running gateway under the
   Component's ``data`` resource.

.. test:: Freeze-frame end-to-end through the fault-detail endpoint
   :id: TEST_0918
   :status: implemented
   :verifies: REQ_0929, REQ_0927, REQ_0915

   A gateway unit test asserts that a fault carrying environment data in the
   snapshot surfaces a non-empty ``snapshots`` array with populated
   ``extended_data_records`` under ``fault_detail``, while a fault without falls
   back to the occurrence-only shape (back-compat). An end-to-end live-server
   test drives the connector binding ``Up → Degraded → Down`` so a DTC confirms,
   then ``GET``\s ``…/components/{id}/faults/{fault_code}`` over real TCP and
   asserts the response carries the freeze-frame under ``snapshots`` /
   ``extended_data_records``.

.. test:: Trigger subscription CRUD round-trip
   :id: TEST_0919
   :status: implemented
   :verifies: REQ_0933

   Over a live axum server, ``POST /api/v1/triggers`` with an entity/severity body
   returns ``201`` carrying a generated id; ``GET /api/v1/triggers`` lists the new
   trigger; ``GET /api/v1/triggers/{id}`` fetches it; ``DELETE
   /api/v1/triggers/{id}`` returns ``204`` and a subsequent ``GET`` of that id
   returns ``404``. A unit test additionally pins the minimal filter semantics:
   a trigger matches by entity id and by a severity floor, and a no-filter
   trigger matches every event.

.. test:: Refresh-diff loop streams fault events as SSE
   :id: TEST_0920
   :status: implemented
   :verifies: REQ_0930, REQ_0931, REQ_0934

   A provider whose snapshot changes between polls (healthy → faulted → healthy)
   feeds a live server running the refresh-and-diff loop. A client registers a
   trigger, connects to ``GET /api/v1/triggers/events``, and observes a
   ``fault_raised`` frame followed by a ``fault_cleared`` frame whose data object
   carries ``event_type``, the ``fault`` sub-object (``fault_code``, numeric
   ``severity``), ``timestamp``, and ``x-medkit`` (``entity_id``,
   ``entity_type``) — the captured golden shape. A unit test pins the diff:
   identical views emit nothing; a newly-present fault raises and a vanished
   fault clears.

.. test:: Health transition streams health_changed
   :id: TEST_0921
   :status: implemented
   :verifies: REQ_0932

   Driving the provider from healthy to error-faulted while a no-filter trigger
   is registered yields a ``health_changed`` SSE frame on the event stream; a
   unit test confirms the diff emits ``health_changed`` carrying the fault that
   drove the transition when an entity's worst-wins health level moves.

.. test:: Token endpoint issues a contract-shaped JWT
   :id: TEST_0922
   :status: implemented
   :verifies: REQ_0935, REQ_0936

   A live axum server over the mock provider answers ``POST /api/v1/auth/token``
   with a ``client_credentials`` body with HTTP ``200`` and a contract-shaped
   ``AuthTokenResponse``: ``token_type`` is ``"Bearer"``, ``expires_in`` is an
   integer, ``scope`` is a string, and ``access_token`` is a JWT-shaped string of
   three non-empty dot-separated ``base64url`` segments. ``POST`` to
   ``/auth/authorize`` likewise issues a token and ``/auth/revoke`` returns a
   ``status``; a ``GET`` on ``/auth/token`` is ``405`` (POST-only). A unit test
   asserts the permissive authenticator issues the JWT-shaped token, echoes the
   requested scope, and the ``base64url`` encoder matches the RFC 4648 vectors.

.. test:: Enforcement = none accepts requests with or without a token
   :id: TEST_0923
   :status: implemented
   :verifies: REQ_0938

   Over a live server, a read-core ``GET`` succeeds with HTTP ``200`` both with no
   ``Authorization`` header and with an unverifiable ``Bearer`` token, proving the
   gateway never rejects a resource request on authentication grounds in v1.

.. test:: Full client login-to-read flow and the Authenticator seam
   :id: TEST_0924
   :status: implemented
   :verifies: REQ_0939, REQ_0937

   An end-to-end test ``POST``\s ``client_credentials`` to ``/api/v1/auth/token``
   over real TCP, extracts the ``access_token``, and calls a read-core endpoint
   presenting it as a ``Bearer`` credential, asserting HTTP ``200``. A second test
   substitutes a strict, externally-defined ``Authenticator`` that rejects all
   credentials via ``router_with_authenticator`` — without touching any handler —
   and shows token issuance now returns ``401`` while the enforcement = none
   read-core still answers ``200``; a unit test confirms the trait is object-safe
   and a rejecting impl drops in behind ``dyn Authenticator``.

.. test:: Lock acquire-extend-release round-trip
   :id: TEST_0925
   :status: implemented
   :verifies: REQ_0940, REQ_0941

   Over a live axum server, ``POST .../components/{id}/locks`` with an
   ``X-Client-Id`` header and an ``AcquireLockRequest`` body returns ``201`` and a
   ``Lock`` whose ``owned`` is true and whose ``lock_expiration`` is an absolute
   RFC3339 instant; ``PUT`` extends it (``204``) and ``DELETE`` releases it
   (``204``), after which a fresh acquire by another client succeeds. A registry
   unit test drives TTL expiry against an injected clock: a held lock
   auto-releases once advanced past its TTL and the resource is then
   re-acquirable, and the formatted ``lock_expiration`` equals ``now + ttl``.

.. test:: Lock conflict and break_lock override
   :id: TEST_0926
   :status: implemented
   :verifies: REQ_0940, REQ_0942

   Over a live server, a second client acquiring a held lock without
   ``break_lock`` receives ``409``; the same acquire with ``break_lock: true``
   evicts the incumbent and returns ``201`` with ``owned`` true. A registry unit
   test confirms the eviction at the store level.

.. test:: Lock ownership and X-Client-Id enforcement
   :id: TEST_0927
   :status: implemented
   :verifies: REQ_0943, REQ_0944

   Over a live server, a ``POST`` with no ``X-Client-Id`` header is ``400`` and a
   non-owner ``PUT`` extend of a live lock is ``409``. A registry unit test
   confirms ownership is enforced on extend and release, that a wrong lock id is
   ``404``, and that the registry is in-memory and off the control path.

.. test:: Root advertises the served surface honestly
   :id: TEST_0936
   :status: implemented
   :verifies: REQ_0965

   A unit test over ``root_document`` asserts every served family
   (data-access, discovery, faults, authentication, locking, triggers,
   vendor-extensions) advertises ``true`` and every deferred family ``false``,
   and that the endpoint catalogue lists the global stream and clear-all. A live
   server confirms the same root over the wire, including the entity-trigger,
   lock, and auth catalogue entries.

.. test:: Health carries the golden telemetry blocks
   :id: TEST_0937
   :status: implemented
   :verifies: REQ_0967

   A unit test asserts ``health_document`` carries the ``x-medkit-entity-cache``
   (with real counts plus ``capacity``), ``x-medkit-data-provider``, and
   ``x-medkit-subscription-executor`` blocks field-complete. Over a live server
   the response additionally carries a numeric ``timestamp`` stamped at the HTTP
   edge.

.. test:: Global fault clear-all acknowledges
   :id: TEST_0938
   :status: implemented
   :verifies: REQ_0964

   Over a live server, ``DELETE /api/v1/faults`` returns ``204``.

.. test:: Auth disable is 404, not 501
   :id: TEST_0939
   :status: implemented
   :verifies: REQ_0968

   Over a live server with auth enabled (default), ``POST /api/v1/auth/token``
   issues a ``200`` token; with ``auth_enabled = false`` the same call returns a
   contract-shaped ``404`` (``error_code: not-found``), distinct from the ``501``
   deferred fallback.

.. test:: Lock reads expose the owner view
   :id: TEST_0940
   :status: implemented
   :verifies: REQ_0963

   Over a live server, after acquiring a lock as ``alice``, ``GET …/locks``
   returns a one-item collection with ``owned: true``; a ``GET`` of the detail
   with no ``X-Client-Id`` returns ``owned: false``; an unknown lock id is
   ``404``. A registry unit test confirms list/detail surface the live lock with
   the owner-derived flag, echo the scopes, and omit an expired lock.

.. test:: Entity-scoped triggers are pinned and isolated
   :id: TEST_0941
   :status: implemented
   :verifies: REQ_0962

   Over a live server, ``POST /api/v1/apps/{id}/triggers`` pins ``entity_id`` to
   the path entity regardless of the body; the entity-scoped list returns it; a
   fetch of that trigger under a different entity is ``404``; the owning entity
   fetches (``200``) and deletes (``204``) it. A store unit test confirms
   entity-scoped listing and in-place update.

.. test:: Global stream replays the ring and honours Last-Event-ID
   :id: TEST_0942
   :status: implemented
   :verifies: REQ_0961, REQ_0966

   Over a live server driven by a provider whose snapshot changes between polls,
   a fault raised before any client connects is replayed from the ring on a
   fresh connect to ``/api/v1/faults/stream`` (no trigger registered — the global
   stream is unfiltered). A reconnect with a ``Last-Event-ID`` past every
   retained id suppresses the replay.

.. test:: Operations catalogue lists and details
   :id: TEST_0943
   :status: implemented
   :verifies: REQ_0969

   Over a live server whose write seam is a configured ``SimActionSink``,
   ``GET …/operations`` returns the configured operation in a collection
   envelope and ``GET …/operations/{op}`` returns its detail; a resource with no
   configured operations returns an empty catalogue. A provider unit test
   confirms the catalogue gates ``start_operation`` (an unknown op is
   ``NotFound``).

.. test:: Operation execution lifecycle over HTTP
   :id: TEST_0944
   :status: implemented
   :verifies: REQ_0970

   Over a live server, ``POST …/operations/{op}/executions`` returns ``202`` with
   a completed execution whose ``result`` echoes the request args; ``GET`` polls
   it; the executions list contains it; ``DELETE`` returns ``204`` and a
   subsequent ``GET`` is ``404``. A provider unit test confirms the same
   start → list → get → cancel lifecycle at the sink level.

.. test:: Unknown operation is 404
   :id: TEST_0945
   :status: implemented
   :verifies: REQ_0970

   Over a live server, both ``GET …/operations/{unknown}`` and
   ``POST …/operations/{unknown}/executions`` return ``404``.

.. test:: Configurations lifecycle over HTTP
   :id: TEST_0946
   :status: implemented
   :verifies: REQ_0971

   Over a live server, ``PUT …/configurations/{id}`` upserts a value (``200``),
   ``GET`` reads it back, the list shows it, a second ``PUT`` updates it,
   ``DELETE`` one returns ``204`` then ``GET`` is ``404``, and ``DELETE`` all
   returns ``204``. A provider unit test confirms the upsert/get/delete and
   delete-all behaviour at the sink level.

.. test:: Bulk-data lifecycle over HTTP
   :id: TEST_0947
   :status: implemented
   :verifies: REQ_0972

   Over a live server, ``POST …/bulk-data/{category}`` uploads raw bytes
   (``201`` with a descriptor), the category and descriptor lists show it, the
   file downloads with the bytes round-tripping intact, and ``DELETE`` returns
   ``204`` after which the download is ``404``. A provider unit test confirms the
   upload/download/delete cycle at the sink level.

.. test:: Scripts lifecycle over HTTP
   :id: TEST_0948
   :status: implemented
   :verifies: REQ_0973

   Over a live server, a script uploads (``201``), is fetched and listed, an
   execution starts under it (``202`` completed), is polled and removed
   (``204`` → ``404``), the script deletes (``204`` → ``404``), and starting an
   execution under an unknown script is ``404``. A provider unit test confirms
   the upload-and-execute lifecycle at the sink level.

.. test:: Software-update lifecycle over HTTP
   :id: TEST_0949
   :status: implemented
   :verifies: REQ_0974

   Over a live server, an update registers (``201``, status ``registered``), is
   fetched/listed, its status reads back, ``prepare`` and ``execute`` transition
   it (``202`` → ``prepared`` → ``executed``), ``DELETE`` returns ``204`` then
   ``GET`` is ``404``, and a transition on an unknown id is ``404``. A provider
   unit test confirms the register/prepare/execute/delete cycle.

.. test:: Lifecycle-status transitions over HTTP
   :id: TEST_0950
   :status: implemented
   :verifies: REQ_0975

   Over a live server, ``GET …/status`` on a fresh entity is ``running``,
   ``PUT …/status/shutdown`` returns ``202`` and transitions to ``stopped``, and
   ``PUT …/status/start`` returns ``202`` and transitions back to ``running``. A
   provider unit test confirms the transition mapping and that an unknown
   transition is a ``BadRequest``.

.. test:: Logs read and configuration over HTTP
   :id: TEST_0951
   :status: implemented
   :verifies: REQ_0976

   Over a live server backed by a ``MockProvider`` carrying log entries of
   differing severities/contexts, ``GET …/logs`` returns all entries,
   ``?severity=error`` narrows to error entries, and ``?context=<substr>`` narrows
   by substring; ``GET …/logs/configuration`` returns a default config and a
   ``PUT`` round-trips a new one. A provider unit test confirms the configuration
   default-and-upsert at the sink level.

.. test:: Cyclic-subscriptions CRUD and sampling SSE
   :id: TEST_0952
   :status: implemented
   :verifies: REQ_0977

   Over a live server, a cyclic subscription round-trips (POST pinned to the
   entity → ``201``, GET, list, PUT, cross-entity GET → ``404``, DELETE → ``204``
   then GET → ``404``), and opening ``…/{sub_id}/events`` yields at least one
   ``data`` sample frame carrying the sampled entity data within a short window.
   Store unit tests confirm entity-scoped listing and pinning.

.. test:: Health telemetry overlay
   :id: TEST_0953
   :status: implemented
   :verifies: REQ_0978

   Over a live server backed by a ``MockProvider::with_telemetry(...)``,
   ``GET /health`` surfaces the provider's override values (e.g. ``pool_cap``,
   ``worker_alive``, ``generation``) while the live entity-cache ``apps`` count
   stays authoritative; a server over a plain provider still returns the zero
   baseline. View unit tests confirm the overlay and the back-compatible default.

.. test:: Single-entity capability catalogue
   :id: TEST_0954
   :status: implemented
   :verifies: REQ_0979

   Over a live server, ``GET /apps/{id}`` carries a ``capabilities`` array whose
   names include the app's sub-resources (data, operations, configurations,
   faults, logs, bulk-data, cyclic-subscriptions, triggers) each with an
   entity-scoped ``href``, the flat per-sub-resource href keys, and the
   ``_links`` self/collection; ``GET /functions/{id}`` exposes the narrower
   function set (no ``locks``/``status``).

.. test:: Build identity in the version catalogue
   :id: TEST_0955
   :status: implemented
   :verifies: REQ_0980

   Two checks. (1) Over a live gateway with an injected ``BuildInfo``,
   ``GET /api/v1/version-info`` carries under ``vendor_info`` the string fields
   ``git_sha``, ``git_short``, ``git_describe``, ``build_timestamp``, and
   ``rustc_version`` plus a boolean ``git_dirty``, each typed as specified, with
   the existing ``version`` unchanged; ``TEST_0906``'s golden shape-match still
   passes, since the fields are additive under ``vendor_info``. (2) A
   ``BuildInfo`` captured with git metadata absent yields ``"unknown"`` for every
   git-derived field and ``false`` for ``git_dirty`` without panicking, holding
   the no-``.git`` fallback of :need:`REQ_0980`.
