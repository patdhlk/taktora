PREEMPT_RT validation harness
=============================

Detailed design for the **PREEMPT_RT validation harness** sub-feature
(:need:`FEAT_0022`). The harness is packaged as an out-of-tree cargo
bin and consumes the :need:`FEAT_0021` telemetry push channel as its
sole measurement path. This page also carries the cycle-overrun fault
primitive (:need:`FEAT_0018`) and the framework internal-fault model
(:need:`FEAT_0024`) design.

.. arch-decision:: Harness as xtask, not CI gate
   :id: ADR_0061
   :status: open
   :refines: REQ_0112

   **Context.** :need:`REQ_0110` requires a documented worst-case
   jitter envelope. The natural ASPICE / industrial pattern is to wire
   a benchmark gate into CI so regressions block merge. Cloud
   GitHub-hosted runners do not run PREEMPT_RT and cannot be made to
   do so without self-hosting. A self-hosted PREEMPT_RT runner for a
   single-maintainer personal project carries ongoing infra cost
   (host availability, kernel updates, runner-agent updates).

   **Decision.** Package the harness as an out-of-tree cargo bin
   under ``xtask/preempt-rt/`` and document a manual reproduction
   procedure (per :need:`REQ_0112`). Do not gate CI on jitter
   measurements. The envelope artifact (:need:`REQ_0110`) is updated
   manually after a measurement run.

   **Alternatives considered.**

   * *Self-hosted PREEMPT_RT runner with auto-gate.* Captures
     regressions automatically but introduces a single-point-of-
     failure infra dependency. Rejected for the current
     single-maintainer setup; revisitable once the project has
     persistent infrastructure.
   * *Scheduled (nightly) run on self-hosted runner.* Same infra
     dependency as the auto-gate, with slower regression detection.
     Rejected for the same reason.
   * *Run ``cyclictest`` only, no harness.* Loses the link between
     measurements and the ``taktora-executor`` dispatch path. Rejected
     because the relevant question is "what jitter does taktora add on
     top of the kernel?", which ``cyclictest`` alone cannot answer.

   **Consequences.**

   ✅ Zero ongoing infra cost; runs are on-demand by the maintainer.
   ✅ The harness path is identical to the production telemetry path
   (per :need:`REQ_0113`), so the manual run is representative of
   production behaviour.
   ❌ Regressions can land between manual runs. Mitigated partly by
   :need:`TEST_0194` (allocation-free telemetry update) and
   :need:`TEST_0192` (overrun counter correctness) staying in regular
   CI; what the harness uniquely validates is the *absolute envelope*,
   not behavioural correctness.

.. arch-decision:: Motion-flavored adapted reference workload
   :id: ADR_0064
   :status: open
   :refines: REQ_0111

   **Context.** :need:`REQ_0111` requires a representative, repeatable
   load profile for the jitter harness. The recognised prior art is the
   ROS 2 real-time working group reference system: a fixed,
   version-controlled node graph (sensor / transform / fusion / cyclic /
   command archetypes) with a designated hot path, a per-node CPU
   calibration tool, and a defined KPI set (hot-path latency, cyclic-node
   period jitter, dropped samples). Two postures: a faithful port of
   that graph (so taktora numbers compare apples-to-apples with published
   reference-system results), or an adapted graph shaped for motion
   control.

   **Decision.** Adapt, do not faithfully port. Reuse the reference
   system's *node archetypes*, *KPI definitions*, and *per-node CPU
   calibration* methodology, but lay out a smaller topology shaped like a
   motion-control application (a cyclic NC-style node on the hot path,
   feeding setpoints; auxiliary sensor/fusion nodes off the hot path).

   **Alternatives considered.**

   * *Faithful port of the full reference-system graph.* Yields direct
     cross-framework comparability ("taktora executor vs other executors
     on the standard graph"). Rejected as the primary harness because the
     graph is autonomy-perception-shaped, not motion-shaped; the hot path
     and node mix do not resemble a taktora motion deployment, so the
     headline numbers would not characterise the load taktora actually
     runs.
   * *Bespoke topology from scratch, no reference-system lineage.*
     Maximum freedom, but discards the reference system's hard-won KPI
     definitions and calibration discipline and invites
     ad-hoc/unrepeatable load. Rejected.

   **Consequences.**

   ✅ The measured load resembles a real taktora motion deployment, so
   the envelope is meaningful for the product's actual use.
   ✅ KPI definitions and per-node calibration are inherited, keeping the
   harness rigorous and tier-portable.
   ❌ Numbers are **not** directly comparable to published
   reference-system executor results (the graph differs). Documented as a
   deliberate trade: domain relevance over cross-framework comparability.

.. building-block:: xtask-preempt-rt harness
   :id: BB_0052
   :status: open
   :implements: REQ_0111
   :refines: ADR_0061, ADR_0064

   Workspace member ``xtask-preempt-rt`` — a cargo bin that constructs
   the motion-flavored reference topology (:need:`ADR_0064`), runs it
   for a configurable number of scan cycles, and writes
   ``CycleObservation`` records to stdout as NDJSON.

   * **Workload.** The reference topology of :need:`ADR_0064` — a fixed
     graph of motion-shaped node archetypes with a designated hot path —
     not an ad-hoc executor. Per-node synthetic CPU work is tuned by a
     ``number_cruncher``-style calibration step so the absolute load is
     comparable across the dev / Pi5 / PREEMPT_RT tiers.
   * **Warm-up.** The first N scan cycles (configurable) are discarded
     before statistics are collected, so cache/page-fault warm-up does
     not contaminate the steady-state envelope.
   * **Usage.** Runs on all three tiers, but as a **local** developer
     tool only — it is never wired as a blocking cloud-CI gate, per
     :need:`ADR_0061`. Cloud runners are neither PREEMPT_RT nor quiet
     enough to measure jitter reliably; the published envelope
     (:need:`REQ_0110`) comes from a manual run on a tuned target.

   CLI shape:

   .. code-block:: text

      cargo xtask preempt-rt-bench \
          --load-profile {idle,cpu-stress,cyclictest-coexist} \
          --cycle-count <N> \
          --task-count <K> \
          --scan-period-us <P>

   The harness installs a custom ``Observer`` implementation whose
   ``on_cycle_stats`` writes one NDJSON line per call. No timing
   measurements are taken outside the ``Observer`` callback
   (per :need:`REQ_0113`).

.. impl:: xtask-preempt-rt — crate layout and procedure doc
   :id: IMPL_0071
   :status: open
   :implements: BB_0052
   :refines: REQ_0111

   **New workspace member ``xtask/preempt-rt/``**

   * ``Cargo.toml`` — depends on ``taktora-executor`` plus minimal
     transitive crates. Not a default workspace build target.
   * ``src/main.rs`` — argument parsing (``clap``), executor
     construction, ``Observer`` wiring, run loop.
   * ``src/workload.rs`` — load-profile fixtures
     (``idle``, ``cpu-stress``, ``cyclictest-coexist``).
     ``cpu-stress`` spawns ``stress-ng``; ``cyclictest-coexist`` prints
     a copy-paste ``cyclictest`` command and waits for the operator.
   * ``src/ndjson.rs`` — minimal NDJSON writer (no ``serde_json``
     dependency to keep the harness's own jitter low).

   **New document ``docs/preempt-rt-procedure.md``** (deferred to
   the implementation phase — written when the first measurement run
   is staged so the procedure can reflect the actual host).

   Sections planned:

   * Prerequisites — Debian / Ubuntu host with
     ``linux-image-rt-amd64`` or equivalent, ``stress-ng``,
     ``rt-tests``.
   * Kernel configuration — ``CONFIG_PREEMPT_RT=y`` verification,
     boot-line flags (``isolcpus=2,3``, ``nohz_full=2,3``,
     ``rcu_nocbs=2,3``).
   * Capability and pinning — ``CAP_SYS_NICE`` requirement for
     ``SCHED_FIFO`` (per :need:`REQ_0041`).
   * Reproducing the envelope — sample command line for each load
     profile.
   * Updating the envelope artifact — how to incorporate fresh
     measurements into :need:`REQ_0110`'s versioned document.

   **Verification**

   * Build + smoke run — :need:`TEST_0240`.
   * NDJSON schema — :need:`TEST_0241`.
   * Push/pull agreement — :need:`TEST_0242`.

Cycle-overrun fault primitive (FEAT_0018)
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

.. building-block:: Cycle-overrun fault primitive surface
   :id: BB_0093
   :status: implemented
   :implements: FEAT_0018

   New module :code:`crates/taktora-executor/src/fault.rs` owning
   :code:`FaultState`, :code:`ExecutorFaultState`, packed
   :code:`AtomicU64` storage, and the post-execute detection hook
   consumed by the executor.

.. impl:: Per-task fault state machine
   :id: IMPL_0081
   :status: implemented
   :implements: REQ_0070, REQ_0102

   Implementation in :code:`crates/taktora-executor/src/fault.rs`
   (FaultAtomic, FaultState, FaultReason) plus the post-execute hook
   in :code:`crates/taktora-executor/src/executor.rs::post_execute_detect_fault`.

.. impl:: Executor-wide fault state machine
   :id: IMPL_0082
   :status: implemented
   :implements: REQ_0071

   Implementation in :code:`crates/taktora-executor/src/fault.rs`
   (ExecutorFaultAtomic, ExecutorFaultState, ExecutorFaultReason)
   plus the executor-wide breach detection in
   :code:`post_execute_detect_fault` and lazy cascade in
   :code:`dispatch_loop`.

.. impl:: Fault state Observer callbacks
   :id: IMPL_0083
   :status: implemented
   :implements: REQ_0073

   Four new :code:`Observer` methods in
   :code:`crates/taktora-executor/src/observer.rs` plus their
   forwards in :code:`crates/taktora-executor-tracing/src/lib.rs`.

.. impl:: Fault handler dispatch path
   :id: IMPL_0084
   :status: implemented
   :implements: REQ_0072

   New :code:`Executor::add_with_fault_handler` registration path and
   :code:`build_handler_job` closure builder in
   :code:`crates/taktora-executor/src/executor.rs`, plus the
   pre-dispatch routing decision in :code:`dispatch_loop`.

----

Framework internal-fault model (FEAT_0024)
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

.. arch-decision:: Abort on framework-invariant violation; watchdog drives outputs safe
   :id: ADR_0065
   :status: open
   :refines: REQ_0123

   **Context.** The cyclic dispatch path has two nested
   :code:`catch_unwind` layers. The **inner** layer
   (:code:`run_item_catch_unwind`, ``executor.rs``) wraps each user
   item and converts a task panic into a ``PanickedTask`` error that
   drives the :need:`FEAT_0018` fault machine — the task-isolation
   guarantee of :need:`AFSR_0004`. The **outer** layer (the pool worker
   loop and inline-submit path, ``pool.rs``) wraps every job and today
   **swallows** whatever it catches (:code:`let _ = catch_unwind(...)`).

   Because user-item panics are already neutralised by the inner layer,
   the *only* panics that can reach the outer layer are framework-
   internal: a poisoned dispatch ``Mutex`` (``first_err``, ``done_cv``,
   ``iter_err``), a ``ready_ring`` overflow, broken in-degree
   accounting. Swallowing these is actively dangerous: e.g. a panic in
   the ``ready_ring.push().expect()`` path leaves ``pending``
   decremented but successors un-enqueued, so :code:`run_once_borrowed`
   spins on its 5 ms ``wait_timeout`` **forever** — a silent cyclic-task
   hang with outputs frozen at their last value and **no fault
   surfaced**, violating :need:`AFSR_0004`. In a control loop a frozen
   actuator is an undefined-state event.

   The runtime stays on ``panic = "unwind"`` globally — the inner
   catch-and-fault mechanism depends on unwinding, so a global
   ``panic = "abort"`` is not an option.

   **Decision.** Treat any panic reaching the outer (framework) boundary
   as a non-recoverable internal-invariant violation and **fail fast**:
   invoke a best-effort, time-bounded user fatal handler
   (:need:`REQ_0125`), then :code:`std::process::abort`. The boundary is
   installed at every runtime-thread top — pool worker loop, inline
   submit, and the executor dispatch thread's run loop. User-item panics
   continue to be caught and faulted at the inner layer (:need:`REQ_0124`),
   never reaching the abort path.

   The **documented output failure model** on abort is: ``abort`` runs
   no destructors (so :code:`EthercatGateway::Drop`'s graceful tokio
   shutdown does *not* run) → the master thread stops emitting
   process-data frames → each output slave's sync-manager watchdog
   expires → the slave drops OP → SAFE-OP and applies its configured
   safe-state values. Outputs hold their last commanded value for up to
   the watchdog timeout, then go safe — with **zero dependency on
   taktora code running after the violation**. This robustness is the
   point: the safe-state path cannot be defeated by the corrupt state
   that triggered the abort. Its load-bearing precondition is
   :need:`AOU_0016` (watchdog enabled, timeout ≤ FTTI/2).

   **Alternatives considered.**

   * *Controlled stop / run the fault handler (REQ_0072) over the broken
     state.* Rejected: once a dispatch invariant is violated the locks,
     ring, and in-degree counters are untrustworthy; executing more
     framework logic over them — including a fault handler — is less
     safe than aborting, and the watchdog already provides the
     output-safe guarantee without it.
   * *Global* ``panic = "abort"``. Rejected: deletes the inner
     catch-and-fault path, collapsing per-task isolation so one task's
     panic kills the whole control process.
   * *Best-effort "drive outputs safe" frame before abort.* Rejected:
     runs master code over state just declared untrustworthy, for a
     guarantee the slave watchdog already provides. (A *narrow*
     last-gasp that does **not** touch executor internals — GPIO pin,
     black-box flush — is permitted via the :need:`REQ_0125` handler.)
   * *Static enforcement of the watchdog bound now.* Initially
     deferred; **implemented 2026-06-07**: the enable bit is decoded
     from the ESI (:need:`REQ_0843`) and validated at network-config
     time, the timeout is resolved against FTTI/2 and validated on the
     quantized effective value (:need:`REQ_0844`, :need:`REQ_0845`),
     and — because real ESI files carry no timeout data and the ESC
     power-on default (100 ms) itself violates the bound — the master
     programs and read-back-verifies the watchdog registers during
     every bring-up and recovery (:need:`REQ_0846`). :need:`AOU_0016`
     now records only the residual assumption (device honours its
     watchdog; safe-state values correct).

   **Consequences.**

   ✅ Infrastructure panics can no longer silently hang the executor;
   they become an immediate, observable process abort.
   ✅ The fail-fast path is exercisable in CI via an injected fatal
   handler (:need:`REQ_0125`), so it does not rot.
   ✅ The output-safe guarantee depends on no post-panic taktora code.
   ❌ The output-safe timing is bounded by the slave watchdog, not by
   taktora; correctness rests on :need:`AOU_0016` holding. Enforcement
   of the ≤ FTTI/2 bound is deferred until the SM watchdog is modelled.
   ❌ ``abort`` skips all destructors process-wide; any non-watchdog
   cleanup (e.g. log flush) must be done in the :need:`REQ_0125`
   handler.

.. building-block:: Framework fail-fast boundary
   :id: BB_0094
   :status: open
   :implements: FEAT_0024

   The outer (framework) panic boundary, realised at every runtime
   thread top: the pool worker loop and inline-submit path in
   :code:`crates/taktora-executor/src/pool.rs`, and the executor
   dispatch thread's run loop in
   :code:`crates/taktora-executor/src/executor.rs`. Each converts a
   caught panic into a call through the registered fatal handler
   followed by :code:`std::process::abort`, replacing today's
   :code:`let _ = catch_unwind(...)` swallow. Carries the
   ``on_fatal`` registration on :code:`ExecutorBuilder` and the
   ``FatalContext`` cause type.

.. impl:: Fail-fast boundary and fatal handler
   :id: IMPL_0085
   :status: open
   :implements: REQ_0123, REQ_0125

   Replace the swallowing :code:`let _ = catch_unwind(...)` in
   ``pool.rs`` (worker loop and inline submit) and wrap the executor
   dispatch thread's run loop, routing a caught payload through
   :code:`Executor`'s registered ``on_fatal`` handler (default no-op,
   itself catch-guarded) then :code:`std::process::abort`. Add the
   ``on_fatal`` builder setter and ``FatalContext`` (captured payload
   message + thread/site label). Function-scoped
   :code:`#[deny(clippy::unwrap_used, clippy::expect_used,
   clippy::panic)]` on the cyclic-path fns, with each intentional
   fail-fast site annotated :code:`#[allow(...)] // fail-fast: <invariant>`.

.. impl:: User-item panic containment
   :id: IMPL_0086
   :status: implemented
   :implements: REQ_0124

   Existing :code:`run_item_catch_unwind` in
   :code:`crates/taktora-executor/src/executor.rs` — retro-documented:
   catches the item panic and builds a ``PanickedTask`` ``ItemError``,
   which the dispatch paths surface via ``Observer::on_app_error`` and
   propagate as the item's error result (stopping downstream items per
   :need:`REQ_0022`). It does **not** drive the :need:`FEAT_0018`
   ``Faulted`` state — that is reserved for deadline breaches
   (:need:`REQ_0070`) — and it never reaches the fail-fast boundary of
   :need:`REQ_0123`.
