Absolute-grid cyclic dispatch
=============================

Detailed design for the absolute-grid cyclic dispatch of :need:`REQ_0268`
(under :need:`FEAT_0011`): the self-computed grid timer, the Linux
``timerfd`` master-tick wake path, the ``Grid`` | ``Legacy`` toggle, and
the scan-count-plus-skip-signal lateness anchoring of :need:`ADR_0101`.

The two dispatch modes differ in how the WaitSet decides when to wake. The
``Legacy`` path arms an iceoryx2 ``attach_interval`` heartbeat and computes
a *relative* ``epoll`` timeout from a ``now`` sampled before the syscall, so
the wakeup→dispatch round-trip ``δ`` leaks into every interval and lateness
accumulates. The ``Grid`` path (production, Linux) blocks on the readiness
of a single absolute ``timerfd`` armed at ``base_period`` and dispatches the
due tasks in a post-wait ``take_due`` pass, immune to the millisecond
rounding of the relative timeout:

.. mermaid::

   flowchart TD
       Mode{"DispatchMode"}

       subgraph Legacy["Legacy (relative timer)"]
           LArm["WaitSet::attach_interval(period)"]
           LWait["epoll timeout = now + period<br/>(now sampled pre-syscall)"]
           LWake["wake at-or-after timeout<br/>δ round-trip uncorrected"]
           LDisp["dispatch on attach_interval callback"]
           LDrift["realized period = period + δ̄<br/>→ lateness accumulates"]
           LArm --> LWait --> LWake --> LDisp --> LDrift --> LWait
       end

       subgraph Grid["Grid (absolute timerfd, Linux)"]
           GEpoch["sample scheduling epoch once<br/>at dispatch_loop entry (CyclicClock)"]
           GArm["arm single master timerfd<br/>TFD_TIMER_ABSTIME @ base_period = gcd"]
           GBlock["WaitSet blocks Duration::MAX on<br/>fd-readiness (attach_notification)"]
           GTick["kernel hrtimer makes fd readable<br/>at exact grid point, auto-rearms"]
           GDrain["drain timerfd (8-byte read)<br/>drain() > 0 gates the pass"]
           GDue["GridTimer::take_due(now):<br/>collect due tasks, skip-realign stalls"]
           GDisp["post-wait pass dispatches each<br/>due task; barrier_and_record"]
           GEpoch --> GArm --> GBlock --> GTick --> GDrain --> GDue --> GDisp --> GBlock
       end

       Mode -->|"Legacy (default non-Linux)"| Legacy
       Mode -->|"Grid (default Linux)"| Grid

.. arch-decision:: Absolute-grid cyclic dispatch via Linux timerfd; self-computed epoll timeout fails on ms-rounding
   :id: ADR_0100
   :status: accepted
   :refines: FEAT_0011
   :links: REQ_0268

   **Context.** The first real-hardware telemetry capture (the
   ``xtask/preempt-rt`` idle harness, :need:`REQ_0111`) showed the cyclic
   dispatch period running systematically long: deadline lateness
   (:need:`REQ_0106`) climbed **linearly without bound** while period jitter
   (:need:`REQ_0101`) stayed tight — the exact jitter-hides-drift case the
   dual-metric telemetry exists to expose. On a Pi5 (``6.12.75+rpt SMP
   PREEMPT``, ``taskset -c 3``, 60 000 cycles @ 1 ms) the slope was
   ~15.3 µs/cycle under ``SCHED_OTHER`` (917 ms/min) and a residual
   ~2.9 µs/cycle under ``SCHED_FIFO`` (176 ms/min) — a real periodic-timer
   component, not schedulability.

   Root cause (iceoryx2 0.8.1 source): cyclic ticks arm via
   ``WaitSet::attach_interval``. The interval anchor on the internal
   ``DeadlineQueue`` is a correct absolute grid, **but** the WaitSet never
   sleeps to an absolute wake target — ``wait_and_process_once`` computes a
   *relative* ``epoll`` timeout from a ``now`` sampled before the syscall, and
   ``epoll`` can only wake at-or-after that timeout (strictly one-sided late).
   The syscall + tick-delivery + callback round-trip ``δ`` is therefore
   uncorrected every cycle, so the realized period is ``period + δ̄`` and
   lateness accumulates. ``attach_interval`` is documented as a *heartbeat*
   cadence, not an absolute-periodicity contract. (Re-checked against
   iceoryx2 0.9.0/0.9.1: the only WaitSet change is "allow ``dyn``
   attachments"; the relative-timeout behaviour is unchanged.)

   **Decision.** Stop using ``attach_interval`` for the grid. Cyclic dispatch
   is phase-locked to an absolute ``CLOCK_MONOTONIC`` grid using a **single
   master Linux ``timerfd``** (``TFD_TIMER_ABSTIME``) armed at
   ``base_period = gcd`` of all declared cyclic periods — one master cycle
   timer per PLC, with every cyclic task phase-locked to it (a task of period
   ``N·base`` fires every ``N``-th tick). It is attached to the WaitSet
   **wake-only**
   (``attach_notification`` via an ``iceoryx2-bb-posix`` ``FileDescriptor``
   wrapper), held separately from the per-task attachment arrays so the callback
   never maps it to a task. The kernel arms an ``hrtimer`` that makes the fd
   readable at the exact grid point and auto-rearms every ``base_period``
   (counting overruns); the master timerfd only **wakes** the WaitSet, and a
   **post-wait ``GridTimer::take_due`` pass dispatches every due cyclic task
   atomically per tick**. The WaitSet blocks (``Duration::MAX``) on
   **fd-readiness** — interrupt-driven, and therefore immune to the ``epoll``
   timeout's millisecond rounding. The loop drains the single master timerfd
   (8-byte read) per wake to clear readiness; ``drain() > 0`` (the grid ticked)
   gates the post-wait pass. The lateness path of :need:`REQ_0106` is left
   untouched.

   This **supersedes** the original self-computed-timeout decision (a
   ``GridTimer`` driving ``wait_and_process_once_with_timeout`` with
   ``min_k(next_k) − now``), which a Pi5 A/B proved cannot bound
   sub-millisecond drift — see Alternatives. On **non-Linux** development hosts
   (no ``timerfd``) ``Grid`` mode keeps that ``GridTimer`` path; those are not
   the real-time target, so its residual drift is immaterial and it preserves
   cross-platform build/test.

   **Alternatives weighed.**

   * *Self-computed absolute timeout* (``GridTimer`` →
     ``wait_and_process_once_with_timeout``). The original decision, **rejected
     on hardware.** iceoryx2's ``epoll`` ``timed_wait`` rounds the timeout **up
     to whole milliseconds** (``iceoryx2-bb-linux`` ``epoll.rs``:
     ``as_nanos().div_ceil(1_000_000)``), so a sub-ms correction (e.g. wake 7 µs
     early to claw back drift) becomes a full 1 ms — quantized away every cycle,
     and ``δ`` accumulates exactly like the relative timer. Pi5 A/B: the grid
     drifted **+3.3 µs/cycle, identical to** ``attach_interval``. No
     self-computed timeout can beat ms-granularity rounding; retained only as
     the non-Linux dev fallback.
   * *Keep* ``attach_interval`` *and add a corrective timeout.* Rejected: the
     ``DeadlineQueue`` anchor is reset only for triggered file descriptors,
     never for pure interval ticks, so a shorter corrective timeout returns
     ``Ok(0)`` with no elapsed deadline — ``handle_deadlines`` fires nothing, so
     we wake but do not dispatch; dispatch stays welded to the drifting anchor.

   **Consequences.** Long-run lateness is bounded. Pi5 FIFO A/B of the
   single master timerfd (``chrt -f 80``, 1 ms, ``cycle_index``-anchored grid):
   timerfd-grid slope **≈ 0 ns/cycle** (0.0001), ``|final lateness|`` **≈ 13 µs
   at 600 000 cycles** — versus legacy ``attach_interval`` **+3.6 µs/cycle /
   2174 ms** (reproducing the bug). Grid jitter is also far tighter (p50
   ~170 ns, p99 ~2.5 µs, vs legacy p50 ~6.5 µs) and held even without core
   pinning. A C-level ``timerfd``+``epoll`` probe independently
   confirmed slope ≈ 0 even under ``SCHED_OTHER``. Linux gains ``libc`` +
   ``iceoryx2-bb-posix`` dependencies. ``Legacy`` (``attach_interval``) is
   retained behind the runtime ``dispatch_mode`` toggle and removed in a
   follow-up; the non-Linux ``GridTimer`` path (and its unit test) likewise
   stay for dev hosts, with retirement deferred. A deadline-miss watchdog
   stays telemetry-only and deferred.

   *Metric caveat.* :need:`REQ_0106`'s ``grid_slot`` reconstruction
   (``round(actual_period / period).max(1)``) over-counts on coalesced
   catch-up wakes, fabricating spurious *negative* lateness; the A/B above uses
   a ``cycle_index``-anchored grid immune to it. Corrected by
   :need:`ADR_0101` (scan-count + skip-signal anchoring).

.. building-block:: Absolute-grid timer and cyclic scheduling clock
   :id: BB_0095
   :status: implemented
   :implements: REQ_0268
   :refines: ADR_0100

   The structural surface that realises :need:`ADR_0100`, in
   ``crates/taktora-executor/src/grid.rs``.

   * **``GridTimer`` — pure state machine** (no clock, no syscalls). Holds a
     scheduling ``epoch`` sampled once at ``dispatch_loop`` entry and a
     per-task slot counter, so the nominal wakeup for scan *k* of a cyclic
     task is ``next_k = epoch + slot_k × period_k`` — an *absolute* grid, not
     a ``now + period`` re-derivation. ``next_timeout(now)`` returns
     ``min_k(next_k) − now`` (saturating to zero), i.e. the distance to the
     earliest pending slot, or ``Duration::MAX`` when no cyclic task is
     registered (empty grid → no grid-driven wakeup). ``take_due(now)``
     collects the tasks whose slot is due and **skip-realigns**: a slot
     starved past one or more whole periods snaps closed-form to the next
     future grid point and dispatches exactly once — it never replays a
     burst of stale cycles, so a transient stall costs bounded slots rather
     than a permanent phase offset. Harmonic multi-period grids share the one
     ``epoch``; coincident slots coalesce in a single ``take_due`` pass.
   * **``CyclicClock`` trait** — the scheduling time source, read as
     ``now_nanos()``. ``MonotonicCyclicClock`` is the ``CLOCK_MONOTONIC``
     implementation. This is **distinct by construction** from the telemetry
     ``MonotonicClock`` that produces the lateness of :need:`REQ_0106`: a
     separate trait, so substituting a test clock for telemetry can never
     alter dispatch timing.
   * **``DispatchMode`` toggle** — ``Grid`` | ``Legacy``, selecting the
     absolute-grid path or the retained ``attach_interval`` path of
     :need:`ADR_0100`. The ``Default`` is **platform-conditional**: ``Grid`` on
     Linux (the production ``timerfd`` path), ``Legacy`` on non-Linux dev hosts
     (where ``Grid`` is only a self-computed-timeout fallback whose ms-rounding
     jitter makes tight timing tests flaky on loaded CI).
   * **``base_period`` / ``gcd`` — pure helpers** that fold all declared cyclic
     periods to the master-tick period (``base_period = gcd`` of the set), so
     one master timerfd phase-locks every cyclic task and a task of period
     ``N·base`` fires every ``N``-th tick.

   Wiring: on **Linux** the single master ``timerfd`` (armed at ``base_period``,
   attached wake-only and drained each wake) drives the wake and the wait blocks
   with ``Duration::MAX``; on **non-Linux** dev hosts (no ``timerfd``) the
   ``GridTimer`` drives — ``next_timeout`` feeds the ``timeout`` argument of
   ``WaitSet::wait_and_process_once_with_timeout`` from ``dispatch_loop``. In
   **both** cases cyclic tasks dispatch in a **post-wait pass** that calls
   ``take_due`` and routes each due task through ``DispatchPass::dispatch_task``,
   folded by ``barrier_and_record``. Event/fd-driven tasks remain on the
   per-attachment callback path; one ``pool.barrier()`` per iteration covers
   both passes.

.. impl:: Grid dispatch wiring and Legacy toggle — executor.rs
   :id: IMPL_0087
   :status: implemented
   :implements: REQ_0268
   :refines: BB_0095

   Concrete changes in ``crates/taktora-executor/src/executor.rs`` that wire
   :need:`BB_0095` into the dispatch loop.

   * **Builder setters** — ``ExecutorBuilder::dispatch_mode`` selects
     ``DispatchMode`` (default is platform-conditional: ``Grid`` on Linux,
     ``Legacy`` on non-Linux); ``ExecutorBuilder::cyclic_clock`` installs a
     ``CyclicClock`` (defaulting to ``MonotonicCyclicClock`` at ``build``).
   * **Grid path owns cyclic timing** — in ``Grid`` mode the loop skips
     ``WaitSet::attach_interval`` for cyclic declarations and instead owns
     them via ``GridTimer``: the epoch is sampled from the ``CyclicClock`` at
     ``dispatch_loop`` entry. On **Linux** the iteration timeout is
     ``Duration::MAX`` and a **single master ``timerfd``**
     (``crates/taktora-executor/src/timerfd.rs``, armed at ``base_period``,
     attached wake-only and drained each wake) drives the wake; on **non-Linux**
     dev hosts ``next_timeout`` computes each iteration's
     ``wait_and_process_once_with_timeout`` timeout. In both cases a post-wait
     pass dispatches the ``take_due`` set through ``DispatchPass::dispatch_task``.
     In ``Legacy`` mode the timeout is also ``Duration::MAX`` and the old
     ``attach_interval`` heartbeat path is used unchanged.
   * **Mode branch hoisted** — ``dispatch_mode`` is read once at
     ``dispatch_loop`` entry into a local, so the per-iteration branch costs
     nothing in the hot loop.
   * **Rejected configurations** — ``validate_decls`` rejects, at add/build
     time, a task declaring both an interval (cyclic) and a listener
     (event-driven) trigger, and a zero-duration interval; a cyclic scan
     period must be strictly positive.
   * **Legacy retention** — the ``attach_interval`` path is retained behind
     the ``DispatchMode`` toggle until the Pi5 A/B of :need:`ADR_0100`
     resolves, then removed in a follow-up.

.. impl:: Run-loop EINTR continue — executor.rs
   :id: IMPL_0088
   :status: implemented
   :implements: REQ_0269

   ``after_callback`` distinguishes iceoryx2's ``WaitSetRunResult``
   variants instead of lumping them: ``TerminationRequest`` (iceoryx2's
   latched SIGINT/SIGTERM) still ends the loop; a bare ``Interrupt``
   (``EINTR`` from any handled signal) funnels past the item-error and
   stop-flag checks and returns ``Continue`` **without counting an
   iteration** — nothing was dispatched on that wake. The grid pass's
   stop-wake suppression already gates on the same variants, so an
   interrupted wake emits no spurious cyclic cycle either.

.. impl:: Dispatch-thread timer slack — executor.rs
   :id: IMPL_0089
   :status: implemented
   :implements: REQ_0274

   ``dispatch_loop`` entry issues ``prctl(PR_SET_TIMERSLACK, 1_000)``
   (Linux-gated). Thread-local; a no-op under real-time scheduling
   classes (the kernel forces RT slack to 0); the return value is
   deliberately unchecked — failure on an exotic kernel merely retains
   the 50 µs default, i.e. today's behavior.

.. arch-decision:: Lateness grid anchored on scan count plus dispatcher skip signal
   :id: ADR_0101
   :status: accepted
   :refines: FEAT_0021
   :links: REQ_0106, REQ_0840

   **Context.** Issue #46: :need:`REQ_0106`'s original grid-slot
   reconstruction (``round(actual_period / period).max(1)``) over-counted
   on a coalesced catch-up pair — a late wake (``actual_period ≈ 1.6 ×
   period``, rounds to 2) followed by a short catch-up (``< period / 2``,
   forced to 1) advances the grid by 3 slots across 2 elapsed periods,
   stamping a permanent ≈ −1 period step into every later cycle's
   lateness. On a Pi5 ``timerfd`` capture whose realized mean period was
   exactly on grid, the reported lateness drifted 0 → −2.006 ms over
   60 000 cycles from two such events; a scan-count-anchored reference
   over the same data showed slope ≈ 0. The witness metric fabricated
   the very drift it exists to expose. A second latent defect: the grid
   epoch was executor-shared (first task to record wins), so every later
   task's start phase read as a permanent constant lateness offset.

   **Decision.** Advance the lateness grid slot by exactly **one per
   scan attempt plus a dispatcher-signalled skip count**
   (:need:`REQ_0840`), and anchor ``grid_epoch`` **per task** at the
   task's own first recorded dispatch, **back-dated by the dispatcher's
   one-shot ``late_by`` signal** so the epoch lands on the first
   dispatch's nominal slot (:need:`REQ_0106`). ``GridTimer::take_due``
   already computes skip-realigns exactly (:need:`BB_0095`); it now
   carries the abandoned-slot count to the task's next dispatch —
   backward-looking, the same row whose fold consumes it — where
   telemetry folds ``grid_slot += 1 + skipped`` — plus, per due entry,
   ``late_by``: the dispatch's distance past the most recent grid point
   at or before it (on a whole-slot miss, the last *passed* lattice
   point — abandoned slots do not exist on the task's own grid, matching
   the first-dispatch carry suppression of :need:`REQ_0840`). All
   timestamps stay on the telemetry clock; exactly two discrete facts
   cross the scheduler→telemetry boundary: the skip count, and the
   first dispatch's ``late_by`` consumed once as the anchor offset.

   **Rejected alternatives.**

   * *Keep the measured-period reconstruction* — conflates per-cycle
     period noise with absolute grid position; the rounding artifact is
     structural, not tunable.
   * *Pure scan-count anchor* (the fix direction proposed on issue #46)
     — exact until the dispatcher genuinely skip-realigns
     (:need:`REQ_0268`), after which every later cycle reports a
     permanent ``+N × period`` lateness: the mirror image of the
     original artifact.
   * *Measure against the dispatcher's own nominal target* — exact by
     construction but collapses the witness: a ``GridTimer`` that drifts
     its targets would self-report zero lateness, precisely what the
     :need:`REQ_0268` independence clause forbids. (Distinct from the
     one-shot ``late_by`` **anchor**: that places the epoch on the
     dispatcher's lattice exactly once, at the first dispatch; every
     subsequent sample still folds telemetry-clock deltas, so a
     target-drifting ``GridTimer`` can shift the constant anchor at most
     and can never zero its own slope.)

   **Consequences.** Coalesced catch-up pairs report one transient
   positive spike and heal; dispatcher skips re-anchor through the
   signal and become observable (``skipped_slots`` — a skipped slot is a
   scan that never ran, previously invisible); ``Legacy`` mode (no
   signal) honestly reports a persistent offset after a missed period
   instead of silently absorbing it. In ``Grid`` mode the epoch
   back-dates by the first dispatch's ``late_by`` (:need:`REQ_0106`), so
   a late process start reads once as real first-cycle lateness — the
   first-sample anchor would otherwise erase it to 0 and invert it into
   a permanent negative floor on every later on-grid cycle (observed on
   the Pi5 rig as a constant −110 µs…−792 µs under loaded starts).
   Without a dispatcher signal (``Legacy``, event-driven) the
   constant-offset blindness of the first-dispatch anchor remains, by
   construction. **Field evidence** (Pi5, ``SCHED_OTHER`` + 6-hog CPU
   starvation, 60 k cycles @ 1 ms, grid dispatch): the corrected metric
   held decile-mean lateness flat across 535 provoked coalesce events,
   while the pre-fix reconstruction staircased −1.4 ms → −30.5 ms over
   645 events on the same load profile; a quiet run's envelope was
   −14.5 µs … +275 µs with zero skips.

.. arch-decision:: Per-phase dispatch dedup via the existing pending_cycle token
   :id: ADR_0105
   :status: accepted
   :refines: FEAT_0017
   :links: REQ_0854, REQ_0002

   **Context.** In released ``taktora-executor`` 0.2.x the per-callback
   ``barrier_and_record`` was doing double duty: besides flushing telemetry
   it was the *only* thing stopping a task's borrowed ``*mut dyn FnMut`` job
   from being submitted twice with no barrier in between. With
   ``worker_threads >= 2`` two ``interval()`` declarations on one task could
   share the grid epoch, come due in the same ``take_due`` pass, and submit
   the **one** borrowed job to **two** pool workers before the single
   barrier — two workers aliasing one ``*mut dyn FnMut``, i.e. undefined
   behavior (a data race on the closure's captured state). This is reachable
   on the production **Grid (Linux-default)** path precisely because
   ``run_grid_cyclic_pass`` barriers only *after* its whole due-loop
   (:need:`BB_0095`), so the protection that masked the hazard was the very
   per-callback barrier the barrier-consolidation slice has now removed (the
   dispatch path runs a single ``barrier_and_record`` per wake).
   This ADR is the in-spec record of that released-0.2.x discrepancy and the
   contract that closes it (:need:`REQ_0854`).

   **Decision.** Promote the **existing** ``pending_cycle`` token to a
   per-phase dedup guard — **no new field**. ``pending_cycle`` is already
   set at dispatch and ``take``\ n only by ``barrier_and_record``, so it is
   exactly a "submitted this phase, no intervening barrier" marker. At the
   top of ``dispatch_task``, **before fault routing**, ``if
   task.pending_cycle.is_some() { return; }`` skips the re-dispatch; the
   listener's pending notifications are level-readable and the job's
   ``take()`` loop drains them, so the single run services every attachment
   fired in the phase. The token is set **uniformly on both the normal and
   the fault-routed branch and for all task kinds**, so the guard also
   covers the borrowed fault-handler submit (the contract is: at most one
   submit — main item *or* fault handler — per task per barrier phase).
   Telemetry stays correct without a special case because ``record_cycle_for``
   already early-returns for event tasks (no ``scan_period``), so only the
   cyclic cycle is recorded and ``cycle_index`` advances at most once per
   phase. Independently, ``validate_decls`` rejects more than one
   ``interval()`` declaration per task at ``Executor::build`` / ``add`` time
   — a behavioral break carried as the 0.2 → 0.3 semver minor bump
   (:need:`REQ_0002`); multi-*listener* tasks stay legal.

   **Rejected alternatives.**

   * *Barrier between each dispatch* (drain the pool after every
     ``dispatch_task`` so a second submit can never alias). Rejected: it
     re-welds correctness to a per-dispatch barrier and **defeats the
     follow-up barrier-consolidation slice** (:need:`BB_0095`'s "one
     ``pool.barrier()`` per iteration"), whose whole point is to amortise
     the barrier across the due-loop. The guard makes consolidation sound
     *without* a barrier per submit.
   * *A separate dedup ``bool`` on ``TaskEntry``.* Rejected as **redundant
     with ``pending_cycle``**: that token already has exactly the set-at-
     dispatch / taken-at-barrier lifetime the guard needs, so a parallel
     flag would be a second source of truth to keep in lock-step (and an
     extra field on the dispatch hot struct) for no added information.

   **Consequences.** The at-most-one-submit contract holds **by
   construction**, not by accident of the per-callback barrier, so the
   barrier-consolidation slice can proceed. A task with two fired
   attachments in one wake-phase now runs **once** (draining all pending
   input via ``take()``) and records one cycle, where 0.2.x ran it twice and
   recorded two — identical behavior for the common single-decl event task.
   The guard is a single ``Option::is_some`` check reusing an existing
   field, so it adds no steady-state allocation (:need:`REQ_0060`). The
   call-site ordering wrinkle (``dispatch_cyclic`` writes
   ``pending_skipped`` / ``pending_late`` *before* ``dispatch_task``) is
   handled deliberately at the call site, accepting the same-task / same-tick
   last-writer value on a dedup-skip.

.. arch-decision:: AttachmentMap — sorted-Vec O(log n) attachment-to-task resolution with lazy-learn dual identity
   :id: ADR_0106
   :status: accepted
   :refines: FEAT_0017
   :links: REQ_0060

   **Context.** ``process_attachment`` resolved every fired id with a linear
   scan over all guards, reconstructing ids inside
   ``has_event_from``/``has_missed_deadline`` each call — O(n) per dispatch
   over the full guard set, on the hot path.

   **Decision.** A single sorted ``Vec<(WaitSetAttachmentId, task_index)>``
   resolved by ``binary_search``, built once after ``build_attachments`` and
   threaded as a short-lived ``&mut`` borrow into the event-dispatch pass.
   Sorted ``Vec`` over ``HashMap``: ids are small ``Copy`` values and
   attachment counts are tens — a cache-resident contiguous array beats a
   SipHash probe.

   **Dual identity / lazy-learn.** A deadline attachment's missed-deadline
   fire is the precomputable ``Deadline``-form id (a binary-search hit); its
   real-event fire is the ``Notification``-form id whose constructor is
   private upstream, so it is learned on first fire via a single linear scan,
   then cached. Negative results are cached too (an ``IGNORE`` sentinel) so
   master-timer / stop-listener wakes cost one failed binary search forever
   after.

   **Deliberate flattening.** ``has_event_from || has_missed_deadline``
   treats a deadline attachment's real event and its miss identically, and the
   map preserves that — both ids resolve to the same bare ``task_index``. The
   two fires *do* arrive as distinct ids (Notification-form vs Deadline-form),
   so routing misses differently in future (e.g. faulting a task on a missed
   deadline — the obvious eventual PLC feature) is a value **enrichment**
   (``(task_idx, FireKind)``), not a redesign. The flattening is a decision,
   not an oversight.

   **Consequences.** Steady-state resolution is O(log n) and allocation-free
   (capacity reserved at ``guards.len() + deadline_count + 2``; a ``resolve``
   capacity ``debug_assert`` turns that reservation into a checked invariant).
   Behavior is identical to the linear scan (same task for the same id).
   Relies on the one-fd-one-attachment / unique-tick-index uniqueness
   invariant. The map's validity — and the soundness of caching ``IGNORE``
   forever — is bounded by the attachment set's **immutability within
   ``dispatch_loop``** (guards built once, no detach, fds stable); any future
   dynamic attach/detach must rebuild or invalidate the map.
