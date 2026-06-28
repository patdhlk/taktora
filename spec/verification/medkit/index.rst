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
   :verifies: REQ_0923, REQ_0924

   A real ``taktora-executor`` with the binding registered as observer and
   monitor runs cyclic App items: the App entity health reflects the lifecycle
   hooks (a running App reads healthy, an erroring App degrades to ``Error``),
   and the readable ``data`` timing (execution count, EWMA, period / rate)
   updates from ``post_execute`` and ``on_cycle_stats``.

.. test:: Hook write path performs zero heap allocation
   :id: TEST_0914
   :status: implemented
   :verifies: REQ_0925

   The binding's hooks are driven in a steady-state loop under a counting global
   allocator; a differential measurement (``count(big) − count(small)``) shows
   zero allocations per hook cycle, with a deliberate-allocation negative case
   proving the counter still observes this thread — mirroring the executor's own
   cycle-stats allocation test and holding :need:`ADR_0111`.

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
