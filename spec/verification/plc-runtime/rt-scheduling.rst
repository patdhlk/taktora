Real-time scheduling
====================

Test cases verifying the real-time worker scheduling sub-feature
(:need:`FEAT_0015`).

.. test:: ThreadAttributes affinity_mask compiles and runs
   :id: TEST_0127
   :status: implemented
   :verifies: REQ_0040

   **Goal.** Under the ``thread_attrs`` cargo feature, a
   ``ThreadAttributes`` carrying an ``affinity_mask(vec![0])``
   passes through ``ExecutorBuilder::worker_attrs`` and the pool
   workers apply it via ``core_affinity::set_for_current``
   without panicking or failing to dispatch.

   **Fixture.** ``crates/taktora-executor/tests/thread_attrs.rs``
   — ``worker_attrs_compiles_and_runs`` (lines 7-29). A
   two-worker executor with attributes
   ``name_prefix("taktora-test")`` and
   ``affinity_mask(vec![0])``; one cyclic item at 10 ms period.

   **Steps.**

   1. Build the attributes; build the executor with
      ``worker_attrs(attrs)``.
   2. Register the cyclic item.
   3. Call ``exec.run_n(1)``.

   **Expected outcome.** The run completes without error; the
   affinity application path is exercised end-to-end on the
   feature-gated build.

.. test:: ThreadAttributes priority setter compiles into the worker thread body
   :id: TEST_0128
   :status: implemented
   :verifies: REQ_0041

   **Goal.** Under the ``thread_attrs`` cargo feature on
   ``target_os = "linux"``, the ``ThreadAttributes::priority``
   setter and the worker thread's ``set_sched_fifo`` call site
   compile and run as part of the worker startup path. Behavioural
   verification of the ``SCHED_FIFO`` policy actually taking
   effect requires ``CAP_SYS_NICE``, which the test host typically
   lacks; ``set_sched_fifo`` swallows the ``EPERM`` and the worker
   continues under ``SCHED_OTHER`` — see the audit note on
   REQ_0041.

   **Fixture.** ``crates/taktora-executor/tests/thread_attrs.rs``
   — ``worker_attrs_compiles_and_runs`` (lines 7-29) exercises
   the ``worker_attrs`` path on which ``set_sched_fifo`` is
   conditionally called.

   **Steps.**

   1. Build a ``ThreadAttributes`` value (the type carries the
      ``priority`` field even when the test does not set it).
   2. Build the executor with the attributes; register one
      cyclic item.
   3. Call ``exec.run_n(1)``.

   **Expected outcome.** The build, executor construction, and
   dispatch all succeed — exercising the compile-time presence
   of the ``priority`` field and the
   ``set_sched_fifo`` call site. Mechanical verification that
   the policy is honoured requires running the host with
   ``CAP_SYS_NICE`` (out of scope for unprivileged CI).
