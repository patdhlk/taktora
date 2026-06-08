Framework internal-fault model
==============================

Gap capability: the runtime distinguishes two classes of in-cycle fault —
a recoverable fault contained at the task boundary, and a non-recoverable
violation of an internal dispatch invariant that fails fast.

.. feat:: Framework internal-fault model
   :id: FEAT_0024
   :status: open
   :satisfies: FEAT_0010

   The runtime distinguishes two classes of in-cycle fault and handles
   them oppositely. A **recoverable** fault — a user item returning an
   error, panicking, or overrunning its deadline — is contained at the
   task boundary and surfaced as a :need:`FEAT_0018` fault transition,
   leaving sibling tasks and the process running. A **non-recoverable**
   fault — a violation of an internal dispatch invariant (lock
   poisoning, ready-ring overflow, broken in-degree accounting) — means
   the executor's own state is unsound; the runtime fails fast rather
   than execute further logic over corrupt state. This feature is the
   runtime realisation of :need:`AFSR_0004` for the panic case.

.. req:: Framework-invariant violation triggers fail-fast
   :id: REQ_0123
   :status: draft
   :satisfies: FEAT_0024
   :links: BB_0094, IMPL_0085, TEST_0823, TEST_0824

   Any panic that escapes the per-item ``catch_unwind`` boundary — i.e.
   a panic originating in framework dispatch machinery rather than in a
   user item's ``execute`` — shall be treated as a non-recoverable
   internal-invariant violation. The runtime shall **not** swallow such
   a panic and shall **not** attempt to continue or resume dispatch.

   On such a violation the runtime shall, in order: (1) invoke a
   user-registered fatal handler (see :need:`REQ_0125`) on a best-effort,
   time-bounded basis; then (2) call :code:`std::process::abort`.
   Because ``abort`` runs no destructors, the output safe-state
   guarantee rests entirely on the external fieldbus watchdog
   (:need:`AOU_0016`), not on any runtime code executing after the
   violation. The fail-fast boundary is realised at every runtime thread
   top — the pool worker loop, the inline-mode submit path, and the
   executor dispatch thread's run loop — since a user-item panic is
   already converted to an error below this boundary and can never reach
   it. See :need:`ADR_0065` for the rationale and the documented failure
   model; this requirement refines the internal-fault-propagation
   obligation of :need:`AFSR_0004`.

   The containment carve-out of :need:`REQ_0124` covers **only** a user
   item's ``execute``. Panics raised in framework-invoked user callbacks
   that run *outside* that inner catch — ``Observer`` and
   ``ExecutionMonitor`` methods (e.g. ``on_app_error``,
   ``post_execute``) — escape to this boundary and therefore fail-fast.
   Integrators shall treat those callbacks as non-panicking.

.. req:: User-item panic is contained, not a fail-fast
   :id: REQ_0124
   :status: implemented
   :satisfies: FEAT_0024
   :links: IMPL_0086, TEST_0825

   A panic originating in a user item's ``execute`` shall be caught and
   converted to an ``ItemError`` (``PanickedTask``). The error shall be
   surfaced to the configured ``Observer`` via ``on_app_error`` and
   propagated as the item's error result — stopping downstream items in
   its enclosing chain or DAG per :need:`REQ_0022` — **without** aborting
   the process, **without** invoking the fatal handler of
   :need:`REQ_0125`, and without affecting independent sibling tasks
   (which continue to be dispatched on subsequent cycles).

   A panicking item does **not** transition the task to the
   ``Faulted`` state of :need:`FEAT_0018`; that state is reserved for
   deadline-budget breaches (:need:`REQ_0070`). Containment here means
   the panic is reified as a normal item error, not escalated to the
   framework fail-fast path. This contained-panic behaviour is
   load-bearing for the task-isolation guarantee of :need:`AFSR_0004`
   and shall not regress to the fail-fast path of :need:`REQ_0123`.

.. req:: User-registered fatal handler
   :id: REQ_0125
   :status: draft
   :satisfies: FEAT_0024
   :links: BB_0094, IMPL_0085, TEST_0823

   The runtime shall accept an optional fatal handler, registered at
   ``Executor::build`` time, invoked once on the fail-fast path of
   :need:`REQ_0123` immediately before :code:`std::process::abort`. The
   default handler is a no-op. The handler contract, which the runtime
   shall document and enforce, is: it runs over known-unsound executor
   state and therefore **must not** access executor internals; it is
   time-bounded; and a panic raised inside the handler shall route
   directly to ``abort`` (the handler is itself catch-guarded). Its
   intended use is a narrow last-gasp — driving a hardware safe-state
   output or flushing a black-box recorder — not recovery.
