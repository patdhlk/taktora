Cycle-time watchdog
===================

Test cases verifying the cycle-time watchdog sub-feature
(:need:`FEAT_0014`).

.. test:: TriggerDeclarer::deadline stores the (listener, deadline) pair
   :id: TEST_0118
   :status: implemented
   :verifies: REQ_0030

   **Goal.** ``TriggerDeclarer::deadline(subscriber, deadline)``
   records a ``TriggerDecl::Deadline { listener, deadline }`` so
   that the dispatch path can later attach a deadline guard and
   surface the missed-deadline condition.

   **Fixture.** In-source test in
   ``crates/taktora-executor/src/trigger.rs`` —
   ``collects_deadline_decl`` (lines 176-187). A
   ``TriggerDeclarer`` constructed via ``new_test()``; a
   ``Subscriber`` built over a unique iceoryx2 service.

   **Steps.**

   1. Create the subscriber and capture its
      ``listener_handle``.
   2. Call ``d.deadline(&sub, Duration::from_millis(50))``.
   3. Pattern-match the first declaration as
      ``TriggerDecl::Deadline { listener, deadline }``.
   4. Assert the stored ``listener`` is ``Arc::ptr_eq`` to the
      expected handle and the stored ``deadline`` equals
      ``Duration::from_millis(50)``.

   **Expected outcome.** The declaration is recorded with both
   fields intact, which is the boundary the executor's
   ``WaitSet::attach_deadline`` consumes.

.. test:: ExecutionMonitor::post_execute reports per-execute duration
   :id: TEST_0119
   :status: implemented
   :verifies: REQ_0031

   **Goal.** ``ExecutionMonitor::post_execute(task, started_at,
   took, ok)`` is called once per dispatch with a non-zero
   ``took`` and the matching ``TaskId``.

   **Fixture.** ``crates/taktora-executor/tests/monitor.rs`` —
   ``monitor_brackets_each_execute`` (lines 26-48). The
   ``RecordingMonitor`` records each ``post_execute`` invocation
   as ``(TaskId, Duration, bool)`` in a ``Mutex<Vec<...>>``.

   **Steps.**

   1. Attach the monitor; register a cyclic item at 10 ms
      period.
   2. Call ``exec.run_n(3)``.
   3. Read the recorded vector.

   **Expected outcome.** Three ``(task, took, ok)`` triples are
   captured, all with ``ok == true``; the post-execute timing
   signature is wired through to every dispatch path.
