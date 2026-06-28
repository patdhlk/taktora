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
