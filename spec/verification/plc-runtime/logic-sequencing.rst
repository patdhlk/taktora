Deterministic logic sequencing
==============================

Test cases verifying the deterministic logic sequencing sub-feature
(:need:`FEAT_0013`).

.. test:: Chain runs its items in declared order
   :id: TEST_0114
   :status: implemented
   :verifies: REQ_0020

   **Goal.** ``Executor::add_chain([head, mid, tail])`` invokes the
   three items strictly in declared order on a single dispatch
   slot per chain invocation.

   **Fixture.** ``crates/taktora-executor/tests/chain.rs`` —
   ``chain_runs_items_in_order`` (lines 8-43). A two-worker
   executor; three items that each push their position number
   (1, 2, 3) into a shared ``Mutex<Vec<u32>>``; the head item
   carries a 10 ms interval trigger so the chain fires once.

   **Steps.**

   1. Register the three items as one chain.
   2. Call ``exec.run_n(1)``.
   3. Read the log vector.

   **Expected outcome.** ``log == vec![1, 2, 3]`` — order is
   preserved across the chain.

.. test:: Diamond DAG runs every vertex exactly once
   :id: TEST_0115
   :status: implemented
   :verifies: REQ_0021

   **Goal.** A four-vertex diamond
   (root → {left, right} → merge) under
   ``Executor::add_graph`` runs each vertex concurrently when its
   in-edges are satisfied and exactly once per triggering cycle.

   **Fixture.** ``crates/taktora-executor/tests/graph.rs`` —
   ``diamond_runs_all_vertices_once`` (lines 8-47). A two-worker
   executor; four vertices, each incrementing its own
   ``AtomicU32``; edges ``r→l``, ``r→rt``, ``l→m``, ``rt→m``;
   root ``r`` carries a 10 ms interval trigger.

   **Steps.**

   1. Build the graph via ``g.vertex(...).edge(r, l).edge(...).root(r)``.
   2. Call ``exec.run_n(1)``.
   3. Read each counter.

   **Expected outcome.** Every counter equals ``1`` — concurrent
   dispatch when in-edges are satisfied, gating until upstream
   vertices have completed.

.. test:: StopChain and Err propagate to downstream items
   :id: TEST_0116
   :status: implemented
   :verifies: REQ_0022

   **Goal.** An item returning ``Ok(ItemFlow::StopChain)`` or
   ``Err`` prevents downstream items in its enclosing chain or
   DAG from being dispatched within the same triggering cycle.

   **Fixture.** Three Rust tests cover the variants:
   ``crates/taktora-executor/tests/chain.rs`` —
   ``stop_chain_aborts_remaining_items`` (lines 45-77) and
   ``err_in_middle_propagates_and_stops`` (lines 79-104); plus
   ``crates/taktora-executor/tests/graph.rs`` —
   ``root_stop_chain_skips_dependents`` (lines 49-72).

   **Steps.**

   1. ``stop_chain_aborts_remaining_items``: head returns
      ``StopChain``; tail increments a counter. After
      ``run_n(1)`` assert ``counter == 1`` (tail did not run).
   2. ``err_in_middle_propagates_and_stops``: mid returns
      ``Err``; tail increments a counter. After ``run_n(1)``
      assert ``tail_seen == 0`` and ``run_n`` returns an error
      whose ``Display`` contains the original ``mid-err`` text.
   3. ``root_stop_chain_skips_dependents``: graph root returns
      ``StopChain``; leaf increments a counter. After
      ``run_n(1)`` assert ``leaf == 0``.

   **Expected outcome.** All three assertions hold — abort
   semantics propagate identically across the chain and graph
   dispatch paths.

.. test:: wrap_with_condition gates execution on the predicate
   :id: TEST_0117
   :status: implemented
   :verifies: REQ_0023

   **Goal.** ``wrap_with_condition(item, predicate)`` runs the
   wrapped item when ``predicate()`` is true and short-circuits
   when it is false.

   **Fixture.** In-source tests in
   ``crates/taktora-executor/src/condition.rs`` —
   ``condition_true_runs_inner`` (lines 67-73) and
   ``condition_false_stops_chain`` (lines 75-81). Each wraps an
   inner ``item`` returning ``Ok(Continue)`` and drives the
   wrapper through a ``ContextHarness``.

   **Steps.**

   1. ``condition_true_runs_inner``: wrap with ``|| true``;
      execute; assert the result is ``ItemFlow::Continue``.
   2. ``condition_false_stops_chain``: wrap with ``|| false``;
      execute; assert the result is ``ItemFlow::StopChain``.

   **Expected outcome.** Both branches behave as documented.
   Note that the false branch surfaces as ``StopChain`` and thus
   also short-circuits the surrounding chain — see the audit
   comment on REQ_0023.
