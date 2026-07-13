Building blocks
===============

The structural decomposition of the PLC runtime: the pre-allocated
dispatch scratch that realises the bounded-time-dispatch zero-allocation
guarantee (:need:`BB_0023`), and the foundation executor surfaces that the
already-shipped foundation requirements refer to.

.. building-block:: Dispatch scratch (pre-allocated)
   :id: BB_0023
   :status: open
   :implements: REQ_0060
   :refines: ADR_0011

   The collection of fields hoisted from per-iteration locals onto
   ``Executor`` and ``Graph`` so that dispatch reuses them. Three
   sub-components:

   * **iter_err slot** — single ``Mutex<Option<ExecutorError>>``
     stored on ``Executor``, reset to ``None`` at the start of
     each ``dispatch_loop`` iteration.
   * **Graph runtime fields** — ``counters: Vec<AtomicUsize>``,
     ``pending: AtomicUsize``, ``first_err: Mutex<Option<...>>``,
     ``stop_flag: AtomicBool``, ``stop_chain_seen: AtomicBool``,
     ``done_cv: (Mutex, Condvar)`` — all stored on ``Graph``,
     reset at the top of ``Graph::run_once``. ``self.successors``
     is borrowed rather than cloned.
   * **Re-dispatch SPSC ring** — bounded, ``Box<[AtomicUsize]>``
     of length ``next_power_of_two(n_vertices)``, owned by
     ``Graph``. Producer = pool worker; consumer = WaitSet
     thread. Used to communicate "vertex ``j`` became ready"
     from worker to scheduler without per-iteration allocation.

   Lifetime contract: every field is created in ``Executor::build``
   (or ``Graph::build`` when the executor builds its graphs) and
   lives for the lifetime of the ``Executor``. Reset semantics —
   not deallocation — drive per-iteration state hygiene.

Foundation building blocks (taktora-executor v0.1)
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

The following building blocks document the executor surfaces that
already exist in ``crates/taktora-executor`` and that the foundation
requirements (:need:`REQ_0001` .. :need:`REQ_0051`) refer to. They
exist as audit-trail targets so the foundation reqs can carry
``:status: implemented`` with a non-empty ``:links:`` to a realising
artefact.

.. building-block:: Cyclic scan trigger and dispatch
   :id: BB_0025
   :status: implemented
   :implements: FEAT_0011

   ``TriggerDeclarer::interval(period)``
   (``crates/taktora-executor/src/trigger.rs``) lets each registered
   item declare a scan period as a ``core::time::Duration``. The
   ``WaitSet`` attaches one interval guard per declaration
   (``executor.rs::WaitSet::attach_interval``) and the dispatch loop
   invokes the item's pre-built closure once per fire. The
   ``ExecutionMonitor`` trait (``src/monitor.rs``) brackets each
   invocation with ``pre_execute`` / ``post_execute`` callbacks so
   scan-cycle execution is observable.

.. building-block:: Iceoryx2 channel surface (Channel / Publisher / Subscriber)
   :id: BB_0026
   :status: implemented
   :implements: FEAT_0012

   ``Channel<T>`` plus the ``Publisher<T>`` / ``Subscriber<T>``
   primitives in ``crates/taktora-executor/src/channel.rs`` wrap an
   iceoryx2 publish-subscribe service. ``Publisher`` exposes three
   send paths — ``send_copy``, ``loan_send``, and ``loan`` — and
   returns ``NotifyOutcome { sent, listeners_notified }`` so the
   sender can detect dropped notifications without an error.
   ``Subscriber::take`` returns a borrowed view of the producer's
   payload (``iceoryx2::sample::Sample``), preserving the zero-copy
   IPC posture. ``TriggerDeclarer::subscriber(&sub)`` wires the
   subscriber's listener handle so a fresh sample wakes the item's
   dispatch.

.. building-block:: Chain and DAG sequencing primitives
   :id: BB_0027
   :status: implemented
   :implements: FEAT_0013

   ``Executor::add_chain`` builds a ``TaskKind::Chain`` whose
   pre-built closure iterates the items in declared order on a
   single pool slot per invocation. ``Executor::add_graph`` builds a
   ``TaskKind::Graph`` whose vertex closures decrement successor
   in-degree counters and dispatch ready successors via the
   ``ReadyRing``. Abort propagation: ``Ok(ItemFlow::StopChain)``
   or ``Err`` in a chain breaks the iterator
   (``executor.rs::build_chain_job``); in a graph the same outcomes
   flip a ``stop_flag`` and short-circuit the subtree
   (``graph.rs::cancel_subtree``). ``wrap_with_condition(item,
   predicate)`` in ``src/condition.rs`` gates execution on a
   runtime-evaluated predicate; a ``false`` predicate returns
   ``Ok(ItemFlow::StopChain)``.

.. building-block:: Deadline trigger and timing monitor
   :id: BB_0028
   :status: implemented
   :implements: FEAT_0014

   ``TriggerDeclarer::deadline(subscriber, deadline)``
   (``src/trigger.rs``) records a per-subscriber deadline; the
   ``WaitSet`` attaches a deadline guard (``attach_deadline``) and
   the dispatch callback honors ``has_missed_deadline`` when the
   guard fires. ``ExecutionMonitor::post_execute(task, started_at,
   took, ok)`` reports actual execute duration per invocation, so
   cycle-time overruns are observable from outside the executor.

.. building-block:: ThreadAttributes (worker affinity and priority)
   :id: BB_0029
   :status: implemented
   :implements: FEAT_0015

   ``ThreadAttributes`` in ``crates/taktora-executor/src/thread_attrs.rs``,
   gated behind the ``thread_attrs`` cargo feature, exposes
   ``name_prefix``, ``affinity_mask: Vec<usize>``, and ``priority:
   Option<i32>`` setters. ``Pool`` workers apply the attributes in
   their thread bodies: ``core_affinity::set_for_current`` pins the
   worker to the configured CPU set, and on ``target_os = "linux"``
   ``set_sched_fifo`` calls ``libc::pthread_setschedparam(...,
   SCHED_FIFO, ...)`` with the configured priority. Failure to apply
   ``SCHED_FIFO`` (typical on processes without ``CAP_SYS_NICE``) is
   silently tolerated — the worker keeps running under the default
   ``SCHED_OTHER``.

.. building-block:: Cooperative shutdown (Stoppable + WaitSet stop wakeup)
   :id: BB_0035
   :status: implemented
   :implements: FEAT_0016

   ``Stoppable`` (``crates/taktora-executor/src/context.rs``) is
   ``Clone`` and carries an iceoryx2 ``Notifier`` wired at
   ``Executor::build`` time. ``Stoppable::stop()`` flips an
   ``AtomicBool`` and calls ``notifier.notify()`` so the WaitSet
   wakes even when no other trigger is pending. The dispatch loop
   (``src/executor.rs``) further matches
   ``WaitSetRunResult::Interrupt`` and
   ``WaitSetRunResult::TerminationRequest`` and returns ``Ok(())``
   so SIGINT / SIGTERM exit the process cleanly.

   The cooperative-shutdown wakeup sequence — a ``stop()`` from any
   thread waking an otherwise-idle WaitSet, and the SIGINT/SIGTERM
   termination path — is:

   .. mermaid::

      sequenceDiagram
          participant C as caller (any thread)
          participant S as Stoppable (cloned)
          participant N as iceoryx2 Notifier
          participant W as WaitSet (dispatch loop)
          participant OS as OS signal

          Note over W: blocked, only a long interval pending
          C->>S: stop()
          S->>S: stop_flag.store(true)
          S->>N: notify()
          N-->>W: wake (notification trigger)
          W->>W: stop_flag observed → return Ok(())

          Note over OS,W: alternative: signal-driven exit
          OS-->>W: SIGINT / SIGTERM (iceoryx2 latches)
          W->>W: match Interrupt / TerminationRequest → return Ok(())
