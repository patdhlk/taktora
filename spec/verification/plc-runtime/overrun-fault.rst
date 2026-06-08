Cycle-overrun fault primitive
=============================

Tests verifying the cycle-overrun fault primitive sub-feature
(:need:`FEAT_0018`).

.. test:: Budget breach faults task and halts dispatch
   :id: TEST_0815
   :status: implemented
   :verifies: REQ_0070, REQ_0102

   Item with :code:`interval(5ms); budget(1ms);` sleeps 3ms in
   :code:`execute()`. After one wakeup: task state is
   :code:`Faulted{BudgetExceeded}`, :code:`overrun_count >= 1`,
   :code:`Observer::on_task_fault` fired exactly once. Subsequent
   wakeups must NOT invoke :code:`execute()` again.

.. test:: Clear task fault resumes dispatch
   :id: TEST_0816
   :status: implemented
   :verifies: REQ_0070

   After the task is Faulted (per :need:`TEST_0815`),
   :code:`clear_task_fault` transitions back to Running. Subsequent
   wakeups invoke :code:`execute()` again.
   :code:`Observer::on_task_clear` fires exactly once. A second breach
   re-fires the full cycle.

.. test:: Iteration budget faults executor with silent cascade
   :id: TEST_0817
   :status: implemented
   :verifies: REQ_0071, REQ_0073

   Executor with :code:`iteration_budget(10ms)`. Two items registered:
   one healthy, one breaching. After one breach: executor state is
   :code:`Faulted{IterationBudgetExceeded}`,
   :code:`Observer::on_executor_fault` fired once, and the healthy
   item transitioned to :code:`Faulted{ExecutorFaulted}` WITHOUT
   per-task :code:`on_task_fault` firing.
   :code:`clear_executor_fault` cascade-clears both, firing
   :code:`on_executor_clear` once and :code:`on_task_clear` per
   cleared task.

.. test:: Fault handler dispatches in place of main item
   :id: TEST_0818
   :status: implemented
   :verifies: REQ_0072

   Item registered via
   :code:`add_with_fault_handler(main, handler)`. After :code:`main`
   breaches budget: subsequent wakeups invoke :code:`handler.execute()`,
   not :code:`main.execute()`. :code:`clear_task_fault` restores main
   dispatch.

.. test:: Overrun count persists across clears
   :id: TEST_0819
   :status: implemented
   :verifies: REQ_0102

   Force a breach, clear, force another breach.
   :code:`overrun_count` is monotonic; not reset by
   :code:`clear_task_fault`.

.. test:: Fault state set from worker visible from main
   :id: TEST_0820
   :status: implemented
   :verifies: REQ_0073

   Multi-worker setup; per-task fault state and
   :code:`overrun_count` set from a pool worker thread are visible
   to the main thread without torn reads or panics.

.. test:: Overrun post-execute path zero allocations
   :id: TEST_0821
   :status: implemented
   :verifies: REQ_0060, REQ_0104

   :code:`CountingAllocator` tracks the steady-state overrun post-execute
   path via the differential-measurement pattern (large vs small
   :code:`run_n`); per-iteration allocs == 0.

.. test:: Fault callbacks forwarded to tracing
   :id: TEST_0822
   :status: implemented
   :verifies: REQ_0073

   :code:`taktora-executor-tracing`'s :code:`TracingObserver` forwards
   :code:`on_task_fault`, :code:`on_task_clear`, :code:`on_executor_fault`,
   :code:`on_executor_clear` to :code:`tracing::warn!` /
   :code:`tracing::info!` on target :code:`taktora.fault` with the
   documented field shape.
