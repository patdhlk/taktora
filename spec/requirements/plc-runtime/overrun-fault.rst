Cycle-overrun fault primitive
=============================

Gap capability: deadline violations transition the runtime — at task or
executor scope — to a configured fault state, rather than only being
reported as timestamps via ``ExecutionMonitor``.

.. feat:: Cycle-overrun fault primitive
   :id: FEAT_0018
   :status: implemented
   :satisfies: FEAT_0010

   Deadline violations transition the runtime — at task or executor scope —
   to a configured fault state, rather than only being reported as
   timestamps via ``ExecutionMonitor``.

.. req:: Per-task overrun fault transition
   :id: REQ_0070
   :status: implemented
   :satisfies: FEAT_0018
   :links: BB_0093, IMPL_0081, TEST_0815, TEST_0816, TEST_0819, TEST_0820, TEST_0821

   When a task's ``execute`` exceeds a configured per-task deadline, the
   runtime shall transition that task to a configured fault state and
   shall not invoke its normal ``execute`` again until cleared.

.. req:: Executor-wide overrun fault transition
   :id: REQ_0071
   :status: implemented
   :satisfies: FEAT_0018
   :links: BB_0093, IMPL_0082, TEST_0817

   When any single dispatch iteration exceeds a configured executor-wide
   deadline, the runtime shall transition the executor to a configured
   fault state.

.. req:: Fault-handler item dispatch
   :id: REQ_0072
   :status: implemented
   :satisfies: FEAT_0018
   :links: BB_0093, IMPL_0084, TEST_0818

   When a task or the executor is in a fault state, the runtime shall
   not run the normal item logic and shall instead dispatch an optional
   user-supplied fault-handler item once per triggering cycle. The
   handler is registered via :code:`Executor::add_with_fault_handler(main, handler)`
   and inherits the main item's triggers (its own
   :code:`declare_triggers` declarations are ignored).

.. req:: Fault state observability
   :id: REQ_0073
   :status: implemented
   :satisfies: FEAT_0018
   :links: BB_0093, IMPL_0083, TEST_0822, TEST_0820

   Fault transitions shall be visible to the configured ``Observer`` via
   a dedicated callback distinct from ``on_app_error`` so users can react
   to overruns separately from item-returned errors.
