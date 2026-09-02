Cyclic scan execution
=====================

Foundation capability (taktora-executor v0.1): periodic execution of a
scheduled item at a configured scan period — the PLC equivalent of a scan
cycle — plus the absolute-grid dispatch, EINTR-immune run loop, and tight
timer-slack guarantees that bound long-run cyclic precision.

.. feat:: Cyclic scan execution
   :id: FEAT_0011
   :status: implemented
   :satisfies: FEAT_0010

   Periodic execution of a scheduled item at a configured scan period —
   the PLC equivalent of a scan cycle.

.. req:: Configurable scan period
   :id: REQ_0001
   :status: implemented
   :satisfies: FEAT_0011
   :links: BB_0025, TEST_0104

   The runtime shall allow each cyclic item to declare a scan period as a
   ``Duration`` via ``TriggerDeclarer::interval(period)``.

.. req:: One execution per scan period
   :id: REQ_0002
   :status: implemented
   :satisfies: FEAT_0011
   :links: BB_0025, TEST_0105, TEST_0872

   Under nominal load (no item exceeding its scan period), the runtime
   shall invoke each cyclic item exactly once per declared period.

   A cyclic task shall declare **exactly one** scan period; multiple
   ``interval()`` declarations on one task are **rejected at
   ``Executor::build`` time** (and at ``add`` / ``add_chain`` time, in
   ``validate_decls``). This makes the at-most-one-submit-per-barrier-phase
   contract of :need:`REQ_0854` unambiguous at the task level and is the
   behavioral break carried as the 0.2 → 0.3 semver minor bump.

.. req:: Scan-cycle execution observability
   :id: REQ_0003
   :status: implemented
   :satisfies: FEAT_0011
   :links: BB_0025, TEST_0106

   The runtime shall emit pre-execute and post-execute timestamps for
   every scan-cycle invocation through the ``ExecutionMonitor`` trait.

.. req:: Absolute-grid cyclic dispatch (bounded long-run lateness)
   :id: REQ_0268
   :status: implemented
   :satisfies: FEAT_0011
   :links: BB_0095, IMPL_0087, TEST_0852

   The runtime shall phase-lock cyclic dispatch to an **absolute** monotonic
   grid: the nominal wakeup for scan *k* of a cyclic item is
   ``epoch + k × period`` against a fixed scheduling epoch sampled once at
   dispatch-loop entry — **not** a target re-derived as ``now + period``
   after each wakeup. Consequently the per-task deadline lateness reported by
   :need:`REQ_0106` shall remain **bounded** — it shall not accumulate
   without bound — over arbitrarily long runs under nominal load.

   This strengthens :need:`REQ_0002` ("exactly once per declared period"):
   firing once per period is necessary but not sufficient, because a
   *relative* interval timer satisfies it while still drifting — the
   per-cycle wakeup→dispatch round-trip leaks into the next interval and the
   grid slides. Firing once per period is required; the grid must also not
   slide.

   A wakeup starved past one or more whole periods shall **skip** the missed
   slots — re-aligning to the next future grid point and dispatching exactly
   once — and shall never replay a burst of stale cycles, so a transient
   stall costs bounded slots rather than a permanent phase offset. Where
   multiple cyclic items declare different periods, every cadence shares the
   one scheduling epoch (a harmonic grid), so all periods phase-align at the
   epoch.

   The scheduling time source shall be **distinct** from the telemetry
   measurement clock that produces the lateness of :need:`REQ_0106`, so
   that substituting a test clock for telemetry can never alter dispatch
   timing. The lateness of :need:`REQ_0106` remains the independent
   witness for this requirement: every timestamp it folds is read from
   the telemetry clock, never from the scheduler. Exactly two discrete
   scheduling facts cross into telemetry: the skipped-slot count of
   :need:`REQ_0840` and the one-shot first-dispatch anchor offset of
   :need:`REQ_0106` — a loud countable event and a single constant, not
   a timing source — so a drifting scheduler can at most shift the
   witness's constant anchor once and still cannot mask its own
   accumulating drift in the witness metric. Verification is by a
   deterministic unit test over the grid state machine — the nominal target
   advances by exactly one period per cycle with zero accumulated offset, and
   a simulated stall skips whole slots; the long-run hardware drift bound is
   recorded as field evidence in :need:`ADR_0100`.

.. req:: Run-loop immunity to spurious wait interruptions
   :id: REQ_0269
   :status: implemented
   :satisfies: FEAT_0011
   :links: IMPL_0088, TEST_0854

   A bare ``EINTR`` of the dispatch loop's blocking wait — any handled
   signal: job control (``SIGSTOP``/``SIGCONT``), a debugger or profiler
   attach, a user-installed handler — shall **not** terminate the run
   loop. The interrupted iteration shall dispatch nothing, count
   nothing, and re-enter the wait. Only an explicit stop request, a task
   error escalation, the active run-mode limit, or a latched termination
   signal (``SIGINT``/``SIGTERM``) may end the loop.

   Rationale: the dispatch loop is the cyclic control plane. Treating an
   interrupted wait as a termination request silently freezes a PLC
   runtime's outputs with exit code 0 — observed on the Pi5 rig, where a
   single ``kill -STOP``/``-CONT`` ended ``run_n(60000)`` cleanly at
   ~5 000 cycles with no diagnostic. ``SIGINT``/``SIGTERM`` termination
   is unaffected: iceoryx2 latches the signal and the loop ends on the
   next wait entry.

.. req:: Tight dispatch-thread timer slack
   :id: REQ_0274
   :status: implemented
   :satisfies: FEAT_0011
   :links: IMPL_0089, TEST_0855

   On Linux the runtime shall set the dispatch thread's timer slack to
   **1 µs** at dispatch-loop entry. ``SCHED_OTHER`` threads inherit the
   kernel's 50 µs default, and the blocking wait's timeout sleep is
   subject to that slack (``epoll_wait`` sleeps through
   ``schedule_hrtimeout_range``), which dominates relative-timer
   (``Legacy``) cyclic precision for non-real-time deployments.

   Field evidence (Pi5, idle, ``SCHED_OTHER``, 1 ms period, 10 k
   cycles): ``Legacy`` dispatch accumulated **56.0 µs/cycle** lateness
   at the default 50 µs slack and **5.5 µs/cycle** at 1 µs slack — the
   residual is iceoryx2's ms-rounded epoll timeout plus wake latency
   (:need:`ADR_0100`). Real-time scheduling classes force slack to 0
   (the same rig under ``SCHED_FIFO`` drifted ~3 µs/cycle) and the
   ``Grid`` ``timerfd`` path is woken by fd readiness rather than a
   timeout, so both are unaffected; the setting removes a 10× cyclic
   precision regression for the non-RT ``Legacy`` case at zero cost
   elsewhere.
