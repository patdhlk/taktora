Cooperative shutdown
====================

Test cases verifying the cooperative shutdown sub-feature
(:need:`FEAT_0016`).

.. test:: Stoppable::stop wakes an idle WaitSet from another thread
   :id: TEST_0129
   :status: implemented
   :verifies: REQ_0051

   **Goal.** A ``Stoppable`` handle cloned *before* ``run()`` —
   which is wired to the executor's notifier at ``build()`` time
   — wakes the WaitSet within a bounded time when ``stop()`` is
   called from another thread, even when the only registered
   trigger is a 60-second interval.

   **Fixture.** ``crates/taktora-executor/tests/stoppable.rs`` —
   ``stop_from_other_thread_wakes_idle_executor`` (lines 12-45).
   An inline-pool executor with one item carrying a 60-second
   interval trigger; a ``Stoppable`` clone obtained before
   ``run()``; a helper thread that sleeps 50 ms then calls
   ``stop()``.

   **Steps.**

   1. Build the executor and register the long-interval item.
   2. Clone ``exec.stoppable()``.
   3. Spawn the helper thread that calls ``stop()`` after
      50 ms.
   4. Call ``exec.run()`` and measure the elapsed time.

   **Expected outcome.** ``run()`` returns in under 2 s — the
   stop notifier wakes the WaitSet promptly rather than
   blocking on the 60-second interval. Quantifies "bounded
   time" empirically at the 2 s ceiling.
