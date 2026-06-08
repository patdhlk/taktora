Soft-RT PLC runtime heart
=========================

This page captures the requirements for using ``taktora-executor`` as the runtime
heart of a soft-real-time PLC. It follows from the gap analysis between
typical PLC architecture (Beckhoff TwinCAT, Siemens TIA, B&R Automation
Studio, Rockwell Logix) and the abstractions taktora-executor provides today.

The decomposition is two-tier:

* **Top-level feature** — :need:`FEAT_0010` — the umbrella capability.
* **Sub-features** — capability themes, each one ``:satisfies:`` the
  top-level feature.
* **Requirements** — concrete shall-clauses that ``:satisfies:`` a
  sub-feature.

Sub-features are grouped into **foundation capabilities** (already provided
by taktora-executor v0.1) and **gap capabilities** (must be added before the
runtime credibly serves as a soft-RT PLC heart). Foundation reqs reference
the existing API surface; gap reqs describe TBD work.

Top-level feature
-----------------

.. feat:: PLC runtime heart on iceoryx2
   :id: FEAT_0010
   :status: open

   A Rust runtime that schedules, sequences, and observes the cyclic
   execution of PLC-style logic (read inputs → run logic → write outputs)
   under soft-real-time constraints, with iceoryx2 as the inter-process
   data plane.

   The runtime targets non-safety industrial automation, robotics control
   loops, and machine-monitoring scenarios. Hard-real-time bounds, safety
   certification, IEC 61131-3 frontends, hot-standby, and specific
   fieldbus protocol stacks are explicitly out of scope; the runtime
   integrates with such concerns but does not implement them.

----

Foundation capabilities
-----------------------

The following sub-features are **already provided** by taktora-executor v0.1.
Their requirements describe the contracts the runtime exposes today; the
work for them is closing the review/approval lifecycle, not authoring new
implementation.

Cyclic scan execution
~~~~~~~~~~~~~~~~~~~~~

.. feat:: Cyclic scan execution
   :id: FEAT_0011
   :status: open
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
   :links: BB_0025, TEST_0105

   Under nominal load (no item exceeding its scan period), the runtime
   shall invoke each cyclic item exactly once per declared period.

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

Event-driven I/O dispatch
~~~~~~~~~~~~~~~~~~~~~~~~~

.. feat:: Event-driven I/O dispatch
   :id: FEAT_0012
   :status: open
   :satisfies: FEAT_0010

   Inter-process inputs and outputs flow through iceoryx2 channels so
   producers wake consumers without polling.

.. req:: Subscriber-triggered ingestion
   :id: REQ_0010
   :status: implemented
   :satisfies: FEAT_0012
   :links: BB_0026, TEST_0107

   The runtime shall trigger an item's ``execute`` whenever a declared
   ``Subscriber<T>`` receives a new sample.

.. req:: Publisher-driven emission
   :id: REQ_0011
   :status: implemented
   :satisfies: FEAT_0012
   :links: BB_0026, TEST_0108

   The runtime shall expose ``Publisher<T>`` send paths (``send_copy``,
   ``loan_send``, ``loan``) for emitting outputs to other processes.

.. req:: Zero-copy IPC transport
   :id: REQ_0012
   :status: implemented
   :satisfies: FEAT_0012
   :links: BB_0026, TEST_0109

   Pub/sub data transfer between processes shall be zero-copy across
   shared memory via iceoryx2; receivers shall obtain a borrowed view of
   the producer's payload, not a deserialised copy.

.. req:: Notification-drop visibility
   :id: REQ_0013
   :status: implemented
   :satisfies: FEAT_0012
   :links: BB_0026, TEST_0113

   The runtime shall surface dropped event-service notifications to the
   sender as a non-error counter (``NotifyOutcome::listeners_notified``)
   so the sender can detect consumer back-pressure programmatically.

Deterministic logic sequencing
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

.. feat:: Deterministic logic sequencing
   :id: FEAT_0013
   :status: open
   :satisfies: FEAT_0010

   Items compose into chains and DAGs with explicit ordering and abort
   semantics — the structural equivalent of a PLC cause-effect network.

.. req:: Sequential chain execution
   :id: REQ_0020
   :status: implemented
   :satisfies: FEAT_0013
   :links: BB_0027, TEST_0114

   The runtime shall execute the items of a chain in declared order on a
   single dispatch slot per chain invocation.

.. req:: Parallel DAG execution
   :id: REQ_0021
   :status: implemented
   :satisfies: FEAT_0013
   :links: BB_0027, TEST_0115

   The runtime shall execute the vertices of a DAG concurrently when their
   in-edges are all satisfied, and shall block downstream vertices until
   all of their upstream vertices have completed.

.. req:: Abort propagation
   :id: REQ_0022
   :status: implemented
   :satisfies: FEAT_0013
   :links: BB_0027, TEST_0116

   An item returning ``Ok(ControlFlow::StopChain)`` or ``Err`` shall
   prevent any downstream items in its enclosing chain or DAG from being
   dispatched within the same triggering cycle.

.. req:: Conditional inclusion
   :id: REQ_0023
   :status: implemented
   :satisfies: FEAT_0013
   :links: BB_0027, TEST_0117

   The runtime shall provide a ``wrap_with_condition(item, predicate)``
   helper that gates an item's execution on a runtime-evaluated predicate.

Cycle-time watchdog
~~~~~~~~~~~~~~~~~~~

.. feat:: Cycle-time watchdog
   :id: FEAT_0014
   :status: open
   :satisfies: FEAT_0010

   Visibility into deadline-missed events at the dispatch layer.

.. req:: Subscriber deadline detection
   :id: REQ_0030
   :status: implemented
   :satisfies: FEAT_0014
   :links: BB_0028, TEST_0118

   The runtime shall provide a ``TriggerDeclarer::deadline(subscriber,
   deadline)`` declaration that fires the item if no event arrives at the
   subscriber within ``deadline``.

.. req:: Per-execute timing visibility
   :id: REQ_0031
   :status: implemented
   :satisfies: FEAT_0014
   :links: BB_0028, TEST_0119

   The runtime shall report each item's actual execute duration through
   ``ExecutionMonitor::post_execute(task, started_at, took, ok)``.

Real-time scheduling
~~~~~~~~~~~~~~~~~~~~

.. feat:: Real-time worker scheduling
   :id: FEAT_0015
   :status: open
   :satisfies: FEAT_0010

   Worker threads can be pinned and prioritized for predictable latency on
   PREEMPT_RT-capable Linux systems.

.. req:: Core-affinity assignment
   :id: REQ_0040
   :status: implemented
   :satisfies: FEAT_0015
   :links: BB_0029, TEST_0127

   The runtime shall, behind the ``thread_attrs`` feature, allow worker
   threads to be pinned to a specified set of CPU cores.

.. req:: SCHED_FIFO priority on Linux
   :id: REQ_0041
   :status: implemented
   :satisfies: FEAT_0015
   :links: BB_0029, TEST_0128

   The runtime shall, behind the ``thread_attrs`` feature on Linux, allow
   worker threads to run under ``SCHED_FIFO`` at a configured priority,
   subject to the process holding ``CAP_SYS_NICE``.

Cooperative shutdown
~~~~~~~~~~~~~~~~~~~~

.. feat:: Cooperative shutdown
   :id: FEAT_0016
   :status: open
   :satisfies: FEAT_0010

   The runtime exits cleanly on signal or programmatic stop without
   leaking worker threads or shared-memory artefacts.

.. req:: Signal-driven shutdown
   :id: REQ_0050
   :status: open
   :satisfies: FEAT_0016

   The runtime shall return cleanly from ``run()`` when SIGINT or SIGTERM
   is delivered to the process, surfacing iceoryx2's ``WaitSetRunResult``
   ``Interrupt`` and ``TerminationRequest`` variants.

.. req:: Programmatic shutdown wakeup
   :id: REQ_0051
   :status: implemented
   :satisfies: FEAT_0016
   :links: BB_0035, TEST_0129

   The runtime shall expose a clonable ``Stoppable`` handle whose
   ``stop()`` method wakes the WaitSet thread within a bounded time even
   when no other trigger is pending.

----

Gap capabilities
----------------

The following sub-features are **not yet provided** by taktora-executor v0.1.
Each is a prerequisite for credibly calling the runtime a soft-real-time
PLC heart. Their requirements are authored at ``status: open`` and
represent work to be planned and executed.

Bounded-time dispatch
~~~~~~~~~~~~~~~~~~~~~

.. feat:: Bounded-time dispatch
   :id: FEAT_0017
   :status: open
   :satisfies: FEAT_0010

   The dispatch hot path shall not allocate, take unbounded locks, or
   block on poll loops, so steady-state cycle latency is bounded by
   factors the runtime declares (not by the system allocator or kernel
   futex implementation).

.. req:: No heap allocation in dispatch
   :id: REQ_0060
   :status: implemented
   :satisfies: FEAT_0017
   :links: BB_0023, IMPL_0001, TEST_0170

   The runtime's dispatch path shall perform zero heap allocations during
   steady-state execution after ``Executor::run`` has been entered. All
   per-iteration data structures (error capture, vertex tracking,
   completion signalling) shall reuse capacity provisioned at
   ``Executor::build`` time.

.. req:: Statically-sized task pool
   :id: REQ_0061
   :status: open
   :satisfies: FEAT_0017

   The runtime's worker pool shall be sized at ``Executor::build`` time
   from a configuration value, and the dispatch path shall not grow or
   shrink the pool during execution.

.. req:: Wait-free completion signalling
   :id: REQ_0063
   :status: open
   :satisfies: FEAT_0017

   The graph DAG scheduler shall not rely on a polling condvar
   ``wait_timeout`` for vertex-completion signalling. Completion shall be
   communicated via a wait-free or bounded-wait primitive whose worst-case
   wakeup latency is documented and dominated by the kernel's wakeup
   delivery latency, not by an internal polling interval.

.. req:: Pre-allocated error slot
   :id: REQ_0062
   :status: implemented
   :satisfies: FEAT_0017
   :links: BB_0023, IMPL_0001, TEST_0141

   The runtime shall capture per-iteration item errors in a pre-allocated
   bounded slot rather than constructing an ``Arc<Mutex<Option<...>>>``
   per dispatch iteration.

Cycle-overrun fault primitive
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

.. feat:: Cycle-overrun fault primitive
   :id: FEAT_0018
   :status: open
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

Mode / state-machine framework
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

.. feat:: Mode / state-machine framework
   :id: FEAT_0019
   :status: open
   :satisfies: FEAT_0010

   A first-class lifecycle for the runtime — distinct from item lifecycle
   — that captures the operational modes typical of PLC programs.

.. req:: Mode lifecycle
   :id: REQ_0080
   :status: open
   :satisfies: FEAT_0019

   The runtime shall support an explicit mode lifecycle of at least
   ``{init, ready, running, fault, stopping, stopped}`` and shall expose
   the current mode through a query API.

.. req:: Mode transition triggers
   :id: REQ_0081
   :status: open
   :satisfies: FEAT_0019

   Mode transitions shall be triggered both programmatically (caller-driven)
   and as a consequence of configured events (executor-wide deadline
   overrun, item error, signal-driven stop).

.. req:: Per-mode task gating
   :id: REQ_0082
   :status: open
   :satisfies: FEAT_0019

   Each registered task shall declare which modes it is enabled in; the
   runtime shall not dispatch a task while it is disabled by the current
   mode.

.. req:: Mode change observability
   :id: REQ_0083
   :status: open
   :satisfies: FEAT_0019

   Mode transitions shall be visible to the configured ``Observer`` via
   a dedicated callback that reports the previous mode, the new mode, and
   the reason for the transition.

Retentive state
~~~~~~~~~~~~~~~

.. feat:: Retentive state
   :id: FEAT_0020
   :status: open
   :satisfies: FEAT_0010

   State that survives process restarts — the equivalent of NVRAM-backed
   retentive memory in classical PLCs.

.. req:: Process-restart persistence
   :id: REQ_0090
   :status: open
   :satisfies: FEAT_0020

   The runtime shall provide a retentive memory abstraction whose declared
   contents persist unchanged across cooperative process restarts.

.. req:: Memory-mapped backing
   :id: REQ_0091
   :status: open
   :satisfies: FEAT_0020

   Retentive memory regions shall be backed by a memory-mapped file with
   a checksum verified at load.

.. req:: Crash-atomic checkpoints
   :id: REQ_0092
   :status: open
   :satisfies: FEAT_0020

   A retentive-memory checkpoint shall be atomic with respect to process
   crash — a concurrent crash shall yield either the pre-checkpoint or
   post-checkpoint contents, never a partial state.

.. req:: Recovery status reporting
   :id: REQ_0093
   :status: open
   :satisfies: FEAT_0020

   At startup, the runtime shall report whether retentive state was loaded
   cleanly, recovered from an incomplete checkpoint (and which version was
   selected), or initialised from defaults because no prior state existed.

Scan-cycle observability
~~~~~~~~~~~~~~~~~~~~~~~~

.. feat:: Scan-cycle observability
   :id: FEAT_0021
   :status: open
   :satisfies: FEAT_0010

   First-class statistics on cycle-time behaviour — percentiles, jitter,
   overrun counts — exposed without requiring users to build their own.

.. req:: Per-task latency percentiles
   :id: REQ_0100
   :status: implemented
   :satisfies: FEAT_0021
   :links: BB_0050, IMPL_0070, TEST_0190

   The runtime shall report p50, p95, and p99 execute-duration percentiles
   per registered task, computed over a sliding window whose size is
   configurable at ``Executor::build`` time.

   Percentile estimation uses a fixed, compile-time bucket histogram so the
   per-sample update path is allocation-free (:need:`REQ_0104`,
   :need:`ADR_0060`). The reported percentile is the **geometric midpoint**
   of the occupied bucket (``2^i · √2`` for the shipped octave layout),
   which removes the systematic downward bias of a lower-edge estimate and
   bounds the estimate's relative error symmetrically at
   ``taktora_stats::PERCENTILE_MAX_REL_ERR_PCT`` — a factor of ``√2``, i.e.
   ≈ +42 % / −29 % for the octave layout.

   These percentiles are **coarse telemetry for trend and shape, not
   threshold-grade figures.** Any SLA, acceptance, commissioning, or
   regression decision shall instead use the exact windowed extremes of the
   exact-extreme SLO conformance gate (:need:`REQ_0851`); the histogram is
   deliberately *not* authoritative for pass/fail. Tightening the percentile
   estimate itself to ≤ 1 % relative error is a separate, deferred concern
   tracked as :need:`REQ_0852` (sub-octave buckets) — it is not on the
   production sign-off path.

.. req:: Exact-extreme SLO conformance gate
   :id: REQ_0851
   :status: implemented
   :satisfies: FEAT_0021
   :links: BB_0050, IMPL_0070, TEST_0849, TEST_0191, TEST_0850

   Any pass/fail decision on cycle-time behaviour — acceptance test,
   commissioning sign-off, regression gate, or alarm threshold — shall be
   evaluated against the **exact** windowed extremes the runtime retains,
   never against the bucket-quantised percentiles of :need:`REQ_0100`. The
   percentile histogram carries up to
   ``taktora_stats::PERCENTILE_MAX_REL_ERR_PCT`` relative error by
   construction and is reserved for trend visualisation.

   The authoritative quantities, each an actual observed sample carrying no
   bucket-quantisation error and aged with the same sliding window as
   :need:`REQ_0100`, are:

   * **execute-duration min/max** — :need:`REQ_0105` (monotonic-deque
     backed, exact);
   * **maximum period jitter** — :need:`REQ_0101` (exact windowed maximum);
   * **maximum deadline lateness** — :need:`REQ_0106` (exact signed windowed
     maximum).

   All three are already exposed on both the pull snapshot and the push /
   NDJSON paths (:need:`REQ_0103`, :need:`REQ_0111`). A jitter SLO of the
   form "max jitter ≤ X over the window" is therefore decidable **exactly,
   today**, with no dependency on the deferred histogram-precision work of
   :need:`REQ_0852`. This requirement records the policy that the exact
   path — not the estimate — is the gate.

.. req:: Sub-octave percentile precision
   :id: REQ_0852
   :status: draft
   :satisfies: FEAT_0021
   :refines: REQ_0100

   To make the percentile **estimate** of :need:`REQ_0100` itself
   threshold-grade, the histogram shall bound percentile relative error at
   ≤ 1 % at bucket centroids across the range 100 ns … 10 s, while
   preserving the allocation-free, bounded-time per-sample update of
   :need:`REQ_0104`. This requires a sub-octave (log-linear /
   mantissa-subdivided) bucket layout in place of the shipped octave
   buckets. Verified by :need:`TEST_0868`.

   Until it lands, exact threshold decisions use the exact-extreme gate of
   :need:`REQ_0851`; this requirement only tightens the estimate and is not
   on the production sign-off path.

   **Math note.** The ≤ 1 % bound and "≥ 3 buckets per decade" are
   *independent* constraints, not equivalent ones. Three buckets per decade
   give ~factor-2 bucket width (≈ 40 % worst-case intra-bucket error); a
   ≤ 1 % centroid bound needs bucket edges in roughly a 1.02 ratio
   (~115 buckets per decade). The superseded wording of :need:`REQ_0100`
   and the original :need:`ADR_0060` consequence note conflated the two.

.. req:: Per-task maximum jitter
   :id: REQ_0101
   :status: implemented
   :satisfies: FEAT_0021
   :links: BB_0050, IMPL_0070, TEST_0191

   The runtime shall report the maximum observed jitter — defined as the
   absolute difference between actual and declared scan period — per
   cyclic task, computed over the same sliding window as
   :need:`REQ_0100`. Lifetime maxima are out of scope; the reported value
   ages out with the window.

.. req:: Per-task overrun counter
   :id: REQ_0102
   :status: implemented
   :satisfies: FEAT_0021
   :refines: REQ_0070
   :links: BB_0093, IMPL_0081, TEST_0815, TEST_0819

   The runtime shall expose a monotonic counter per task that increments
   on each scan-cycle execution that exceeds the declared budget per
   :need:`REQ_0070`. The counter shall not reset on
   :code:`Executor::clear_task_fault`; it tracks lifetime breaches.

.. req:: Statistics query API
   :id: REQ_0103
   :status: implemented
   :satisfies: FEAT_0021
   :links: BB_0051, IMPL_0070, TEST_0193

   Cycle-cycle statistics shall be available via two distinct paths:

   * **Push** — the ``Observer`` trait shall expose an
     ``on_cycle_stats(&CycleObservation)`` callback (provided as a no-op
     default for backward compatibility) that fires once per scan cycle
     (including a faulted scan, see :need:`REQ_0107`) with the raw
     per-cycle observation (``cycle_index``, ``task_id``, ``task_index``,
     ``faulted``, ``period_ns``, ``pre_ns``, ``actual_period_ns``,
     ``jitter_ns``, ``lateness_ns``, ``took_ns``, ``skipped_slots``). The ``cycle_index`` is
     the monotonic scan count of :need:`REQ_0107`, the join key by which a
     cyclic connector's telemetry (:need:`REQ_0265`) composes with the
     executor's. The push path delivers raw samples, not aggregates.

     The observation shall additionally carry ``task_index`` — the task's
     stable zero-based registration index — as a flat ``u32`` identity/join
     key (so a consumer need not hash the ``Arc<str>`` ``task_id`` on the hot
     path) and ``pre_ns`` — the telemetry-clock nanosecond instant of
     task-logic start (the canonical reference point of :need:`REQ_0101`).
     ``pre_ns`` is the single time source for an exported sample's time axis;
     a consumer shall not read a second clock. Both fields are always present
     (never absent), including on a faulted scan.
     ``skipped_slots`` — the dispatcher skip count of :need:`REQ_0840` —
     is likewise always present (``0`` when nothing was skipped),
     including on a faulted scan.

     A faulted scan shall be **distinguishable** from a healthy one, the
     cross-layer twin of the connector's ``CycleOutcome`` (:need:`REQ_0267`):
     the observation shall carry a ``faulted`` flag, and every measured
     quantity (``actual_period_ns``, ``jitter_ns``, ``lateness_ns``,
     ``took_ns``) shall encode "not measured this cycle" as *absent*
     (``Option::None``), never as a measured ``0`` — so a consumer joining
     the executor and connector push streams on ``cycle_index`` sees a
     consistent absent-on-fault signal from both layers rather than an
     ambiguous zero.
   * **Pull** — ``Executor::stats_snapshot()`` shall return a borrowed
     view of the current per-task aggregates (``p50``, ``p95``, ``p99``,
     ``max_jitter_ns``, ``overrun_count``), readable concurrently with
     dispatch.

   Both paths shall be allocation-free on the runtime side (see
   :need:`REQ_0104`); allocations on the consumer side are out of scope.

.. req:: Allocation-free telemetry update
   :id: REQ_0104
   :status: implemented
   :satisfies: FEAT_0021
   :refines: REQ_0060
   :links: BB_0053, IMPL_0070, TEST_0194

   The runtime's per-sample telemetry update path — the code that runs
   inside the dispatch loop's timing hooks to update the histogram,
   max-jitter, and overrun counter — shall perform zero heap allocations
   and shall complete in bounded time.

   The update path's worst-case runtime shall be dominated by the
   histogram bucket-index computation (a ``log2``-style lookup, no loops
   over samples) and atomic updates to the bucket counter plus the
   max-jitter and overrun fields. The verification harness mirrors
   :need:`TEST_0170` (``CountingAllocator`` covering pool worker
   threads); see :need:`TEST_0194`.

.. req:: Per-task exact min/max execute duration
   :id: REQ_0105
   :status: implemented
   :satisfies: FEAT_0021
   :links: BB_0050, IMPL_0070, TEST_0849

   In addition to the bucket-quantised percentiles of :need:`REQ_0100`,
   the runtime shall report the **exact** minimum and maximum
   execute-duration observed per registered task, over the same sliding
   window as :need:`REQ_0100`. "Exact" means the reported values are
   actual observed samples, not bucket centroids — the absolute
   worst-case sample is retained, not merely the top occupied bucket.

   The min/max shall age out with the window (lifetime extrema are out of
   scope, consistent with :need:`REQ_0101`). The implementation shall be
   allocation-free per :need:`REQ_0104`; a fixed-capacity monotonic deque
   (sized to the window at ``Executor::build`` time) is the intended
   mechanism, since the histogram of :need:`ADR_0060` cannot recover an
   exact extremum after ageing-out by snapshot subtraction.

.. req:: Per-task deadline lateness
   :id: REQ_0106
   :status: implemented
   :satisfies: FEAT_0021
   :links: BB_0050, IMPL_0070, TEST_0850, TEST_0856

   For each cyclic task, the runtime shall report **deadline lateness** —
   the signed offset between the task's actual task-logic start (the
   ``pre_execute`` instant) and the nominal periodic grid point at which
   it was due to start — over the same sliding window as
   :need:`REQ_0100`. Positive lateness means the task started late.

   Deadline lateness is distinct from the period jitter of
   :need:`REQ_0101`: jitter captures the spread of the measured period
   and is blind to a constant offset, whereas lateness captures steady
   drift or constant offset from the grid. The reported aggregate shall
   include at least the windowed maximum (most-late) lateness; the raw
   per-cycle ``lateness_ns`` is delivered on the push path of
   :need:`REQ_0103`. Event-driven (non-cyclic) tasks have no declared
   period and therefore report no lateness.

   **Grid anchoring.** The nominal grid point for a cycle is
   ``grid_epoch + grid_slot × period``, where ``grid_epoch`` is the
   task's **own** first recorded dispatch **back-dated by the
   dispatcher's one-shot ``late_by`` signal** — the distance from that
   dispatch to the most recent dispatcher grid point at or before it —
   so the grid anchors at the first dispatch's **nominal slot** and a
   late process start is reported once, honestly, as first-cycle
   lateness instead of becoming a permanent negative floor on every
   later on-grid cycle (a faulted first scan anchors too — its dispatch
   instant is real). ``grid_slot`` advances by
   exactly **one slot per scan attempt plus the dispatcher's
   skipped-slot signal** (:need:`REQ_0840`). The slot is never
   reconstructed from the measured period: rounding
   ``actual_period / period`` over-counts on a coalesced catch-up wake
   (a late cycle followed by a short catch-up cycle), stamping a
   permanent spurious negative step into every later cycle's lateness
   (:need:`ADR_0101`). A steady sub-period slip therefore accumulates
   as intended; a late-but-served wake shows a transient positive spike
   that heals on the catch-up dispatch; a dispatcher skip-realign
   (:need:`REQ_0268`) re-anchors through the signal. Absent a signal —
   the ``Legacy`` relative-timer mode never skips slots — a whole
   missed period honestly remains visible as a persistent offset rather
   than being silently absorbed. Absent the anchor signal likewise —
   ``Legacy`` mode and event-driven dispatch have no dispatcher grid —
   the epoch stays the first recorded dispatch itself, and a constant
   phase offset already present there reads as zero by construction.
   The slot advance is aligned with the
   :need:`REQ_0107` ``cycle_index`` (one per scan attempt) except for
   the explicit skip signal.

.. req:: Per-task scan index and faulted-scan emission
   :id: REQ_0107
   :status: implemented
   :satisfies: FEAT_0021
   :links: IMPL_0070, TEST_0851

   The runtime shall maintain, per cyclic task, a monotonic zero-indexed
   ``cycle_index`` (scan count) incremented once per scan **attempt**, and
   shall include it in the push observation of :need:`REQ_0103`. The
   runtime shall fire ``on_cycle_stats`` and increment ``cycle_index`` on
   **every** scan attempt, including a scan whose task logic returned an
   error or was otherwise faulted — not only completed scans.

   This exists so the executor's telemetry composes with a cyclic
   connector's (:need:`FEAT_0038`): because the NC task fires exactly once
   per bus cycle (one-network-one-process), the executor's per-task
   ``cycle_index`` equals the connector's per-cycle ``cycle_index``
   (:need:`REQ_0265`, :need:`REQ_0267`) for the same cycle, giving a
   consumer an explicit join key. Were the executor to skip a faulted
   scan, its count would lag the connector's from the first fault onward
   and every downstream pairing would desync — so emit-on-fault is
   required symmetrically on both layers. The update is allocation-free
   per :need:`REQ_0104`.

.. req:: Per-task skipped-slot count
   :id: REQ_0840
   :status: implemented
   :satisfies: FEAT_0021
   :links: IMPL_0070, TEST_0853

   For each cyclic task, the runtime shall report per scan cycle the
   number of nominal grid slots the dispatcher passed over **unserved**
   between the slot served by the task's previous dispatch and the slot
   served by this dispatch (the skip-realign of :need:`REQ_0268`). The
   count shall be ``0`` in steady state, ``0`` whenever the dispatch
   mode provides no skip signal (the ``Legacy`` relative timer), and
   ``0`` on a task's first recorded cycle — the lateness grid of
   :need:`REQ_0106` anchors there, so earlier slots do not exist on the
   task's own grid.

   The count shall be carried on the push observation of
   :need:`REQ_0103` (field ``skipped_slots``, always present, never
   ``null``) and on the NDJSON record of :need:`REQ_0111`. The lateness
   grid of :need:`REQ_0106` shall advance by exactly
   ``1 + skipped_slots`` per scan attempt, so a dispatcher skip
   re-anchors the grid without per-cycle reconstruction. Only this
   discrete count crosses the scheduler→telemetry boundary; every
   timestamp remains on the telemetry measurement clock
   (:need:`REQ_0268`). A skipped slot is a scan that never executed —
   previously invisible in the exported stream. The update is
   allocation-free per :need:`REQ_0104`.

PREEMPT_RT validation
~~~~~~~~~~~~~~~~~~~~~

.. feat:: PREEMPT_RT validation harness
   :id: FEAT_0022
   :status: open
   :satisfies: FEAT_0010

   The runtime's worst-case latency on PREEMPT_RT Linux is characterised
   under realistic load — a continuous regression gate, not a one-off
   measurement.

.. req:: Documented worst-case jitter
   :id: REQ_0110
   :status: draft
   :satisfies: FEAT_0022

   The repository shall ship a versioned document
   (``spec/safety/preempt-rt-envelope.rst`` or sibling) recording an
   observed worst-case jitter envelope on at least one PREEMPT_RT Linux
   configuration. The document shall record: kernel version (e.g.
   ``6.6.0-rt8``), isolation flags applied (``isolcpus``, ``nohz_full``,
   ``rcu_nocbs``), CPU model and core-pinning layout, load profile
   selected (see :need:`REQ_0111`), and the observed p50 / p95 / p99 /
   max jitter values per task as reported by :need:`REQ_0100` and
   :need:`REQ_0101`.

.. req:: Cyclictest-style benchmark harness
   :id: REQ_0111
   :status: draft
   :satisfies: FEAT_0022

   The repository shall include a benchmark harness, packaged as a
   cargo binary under ``xtask/preempt-rt/``, that exercises the
   ``taktora-executor`` dispatch path under a configured load profile and
   emits per-cycle latency observations as NDJSON to stdout.

   Each NDJSON record shall be emitted once per scan cycle (including a
   faulted scan, see :need:`REQ_0107`) and shall conform to the schema
   ``{ cycle_index: u64, task_id: u32, faulted: bool, ts_ns: u64,
   period_ns: u64, actual_period_ns: u64|null, jitter_ns: u64|null,
   lateness_ns: i64|null, took_ns: u64|null, skipped_slots: u32 }``. Consistent with the
   absent-vs-zero contract of :need:`REQ_0103`, a measurement not taken this
   cycle (first cycle, or any faulted scan) shall be encoded as JSON
   ``null``, never as a measured ``0``. ``ts_ns`` is the task-logic-start
   instant carried as ``pre_ns`` in :need:`REQ_0103`; ``task_id`` is the
   ``task_index`` of :need:`REQ_0103`. ``skipped_slots`` is always a
   number (never ``null``), per :need:`REQ_0840`.

   The harness shall offer at least three selectable load profiles:

   * ``idle`` — no co-resident load; baseline measurement.
   * ``cpu-stress`` — ``stress-ng --cpu N``-style background load on
     non-isolated cores.
   * ``cyclictest-coexist`` — runs alongside ``cyclictest`` so the two
     measurements can be cross-checked.

   *Implementation status.* The ``idle`` profile and the NDJSON export path
   (the off-RT-thread overwrite-oldest telemetry ring and its drain) are
   implemented in ``taktora-telemetry-export`` and the ``xtask/preempt-rt``
   harness. The ``cpu-stress`` and ``cyclictest-coexist`` profiles remain a
   tracked follow-up; this requirement stays ``draft`` until all three ship.

.. req:: Documented reproducer procedure
   :id: REQ_0112
   :status: draft
   :satisfies: FEAT_0022

   The repository shall ship a documented procedure
   (``docs/preempt-rt-procedure.md``) by which any maintainer with
   access to a PREEMPT_RT-equipped Linux host can reproduce the
   :need:`REQ_0110` envelope. The procedure shall be runnable from a
   single ``cargo xtask preempt-rt-bench`` invocation given a
   pre-installed PREEMPT_RT kernel and the documented isolation flags.

   A continuous CI gate on jitter is explicitly **not** required by
   this REQ. See :need:`ADR_0061` for the rationale (no persistent
   self-hosted RT runner; cloud GH runners cannot guarantee
   PREEMPT_RT).

.. req:: Harness consumes runtime telemetry
   :id: REQ_0113
   :status: draft
   :satisfies: FEAT_0022
   :refines: REQ_0103

   The benchmark harness (:need:`REQ_0111`) shall obtain its per-cycle
   observations exclusively via the ``Observer::on_cycle_stats`` push
   callback defined in :need:`REQ_0103`. The harness shall not
   instantiate a parallel timing path (no direct ``Instant::now``
   polling around ``execute``); what the harness measures is what the
   runtime would report in production.

Fieldbus integration interface
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

.. feat:: Fieldbus integration interface
   :id: FEAT_0023
   :status: open
   :satisfies: FEAT_0010

   The shape by which fieldbus protocol stacks (EtherCAT, Modbus, Profinet,
   CIP) plug into the runtime — without committing to any specific
   protocol implementation in the core.

.. req:: Adapter-driven I/O
   :id: REQ_0120
   :status: open
   :satisfies: FEAT_0023

   The runtime shall expose an adapter trait by which a fieldbus driver
   produces ``Channel<T>`` / ``Subscriber<T>`` bindings for ingested
   process variables and consumes ``Publisher<T>`` for outputs.

.. req:: Out-of-tree driver crates
   :id: REQ_0121
   :status: open
   :satisfies: FEAT_0023

   Fieldbus driver implementations shall live in separate crates and shall
   not require modifications to the executor core.

.. req:: Protocol-neutral runtime
   :id: REQ_0122
   :status: open
   :satisfies: FEAT_0010

   The executor core shall not embed any specific fieldbus protocol
   implementation; protocol selection is a deployment concern carried in
   adapter crates.

Framework internal-fault model
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

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

----

Cross-cutting traceability
--------------------------

Every requirement on this page ``:satisfies:`` exactly one parent feature;
every sub-feature ``:satisfies:`` :need:`FEAT_0010`. The needtables on
:doc:`index` and :doc:`../architecture/index` will populate as ``spec``
artefacts are authored.

.. needtable::
   :types: feat
   :columns: id, title, status, satisfies
   :show_filters:

.. needtable::
   :types: req
   :columns: id, title, status, satisfies
   :show_filters:

Safety refinements
------------------

The PLC runtime (``taktora-executor``) carries four TSRs from the SEooC
safety concept (see :doc:`../safety/tsc`):

* :need:`TSR_0003` (integrity-level declaration and process isolation
  for executable items) — **draft**; ``ExecutableItem`` trait and
  registration API need an integrity-level field. See :need:`ADR_0050`.
* :need:`TSR_0004` (missed-deadline detection within one cycle) —
  **implemented** by the executor's existing deadline monitor.
* :need:`TSR_0009` (cross-process hosting mode) — **draft**; the
  executor must support a mode that hosts only SC items and
  cross-references QM items via iceoryx2.
* :need:`TSR_0010` (heartbeat for Element B monitor) — **draft**; no
  liveness heartbeat surface exists today.
