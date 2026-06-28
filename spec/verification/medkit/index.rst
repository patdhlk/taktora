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
