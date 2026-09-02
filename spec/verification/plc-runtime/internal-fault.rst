Framework internal-fault model (FEAT_0024)
==========================================

Verification for the fail-fast boundary (:need:`REQ_0123`), the user
fatal handler (:need:`REQ_0125`), and the contained-item-panic
guarantee (:need:`REQ_0124`).

.. test:: Framework panic routes to fatal handler
   :id: TEST_0823
   :status: implemented
   :verifies: REQ_0123, REQ_0125

   In-process boundary test. A recording terminal (instead of aborting)
   is installed, and a synthetic non-item panic — :code:`panic!(
   "synthetic infra panic")` — is injected through the boundary. The
   ``PoolWorker`` and ``InlineSubmit`` sites are driven end-to-end
   through a real threaded and inline ``Pool`` respectively; the
   ``ExecutorRunLoop`` site is verified at the ``guard_or_fatal``
   mechanism level (a deterministic run-loop infra panic cannot be
   provoked without an artificial fault-injection seam). Asserts the
   fatal fired exactly once per site with the expected ``FatalSite`` and
   cause. Decouples the boundary check from any specific unreachable
   invariant violation.

.. test:: Default fail-fast aborts the process
   :id: TEST_0824
   :status: implemented
   :verifies: REQ_0123

   Subprocess test. A child process with the **default** (no-op) fatal
   handler triggers a framework-boundary panic; the parent asserts the
   child terminated via ``SIGABRT`` (abnormal, non-unwinding exit), not a
   clean exit and not a hang.

.. test:: Item panic is contained, not aborted
   :id: TEST_0825
   :status: implemented
   :verifies: REQ_0124

   Regression guard for the inner layer. A user item that panics in
   :code:`execute` is caught and surfaced via ``on_app_error`` as a
   ``PanickedTask`` error, **leaves the task in ``Running``** (containment
   is not a fault transition — ``Faulted`` is reserved for deadline
   breaches per :need:`REQ_0070`), leaves independent sibling tasks
   running, and does **not** invoke the fatal handler or abort the
   process — proving the contained-panic path of :need:`REQ_0124` never
   escalates to the fail-fast path of :need:`REQ_0123`.
