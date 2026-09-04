.. _architecture-orchestration:

Process orchestration for taktora — architecture (arc42)
========================================================

Architecture documentation for a **process-orchestration layer above
taktora**, structured per the arc42 template (12 sections) and encoded
with sphinx-needs using the useblocks "x-as-code" conventions
(https://x-as-code.useblocks.com/how-to-guides/arc42/index.html).

taktora deliberately ships **no** supervisor, launcher, or process
manifest: restart policy is *"the host's responsibility, matching
taktora-executor's existing posture"* (see the connector cross-cutting
requirements). This chapter specifies how an integrator wires one or
more taktora processes into a supervised deployment — who launches them,
in what order, how liveness is watched, how faults are recovered, and
how cross-process integrity and fail-safety are preserved.

.. note::

   **Provenance / status.** This is a design proposal authored at
   ``open`` (drafted, not yet reviewed). Every ``req`` here is defined
   inline for a self-contained handoff; when the concept is accepted the
   requirements should migrate to ``requirements/orchestration/`` and the
   building blocks should reference concrete crate IDs. Nothing in this
   chapter is implemented in the workspace today except the taktora-side
   *contract surface* of :need:`BB_0200`, which already exists.

.. contents:: Sections
   :local:
   :depth: 1

----

Top-level feature
-----------------

.. feat:: Supervised multi-process deployment of taktora
   :id: FEAT_0200
   :status: open

   An orchestration layer that launches, sequences, supervises, and
   recovers one or more taktora-executor OS processes as a coordinated
   deployment, using taktora's existing lifecycle/health contract as the
   sole coupling surface.

   This is an **integration-layer** feature. It sits *above* the runtime
   heart (:need:`FEAT_0010`) and the connector framework
   (:need:`FEAT_0030`); it is explicitly **not** part of the executor
   core, which stays a single-process reactor. Two realisations are in
   scope: reuse of an existing init/supervisor (systemd) and a bespoke
   taktora-native supervisor ("conductor").

1. Introduction and goals
-------------------------

The reason-to-exist is a gap, not a feature request against the
executor. A taktora *process* — one :code:`Executor` (one WaitSet
dispatch thread) plus its connectors and one iceoryx2 ``Node``, pinned to
a single ``IntegrityLevel`` — is a well-behaved *orchestratee*: it exits
cleanly on signal (:need:`REQ_0050`), stops on a ``Stoppable`` handle
(:need:`REQ_0051`), gates its own cold start behind an admission check
(:need:`TSR_0011`), emits a bounded-period heartbeat (:need:`TSR_0010`),
and drives its outputs safe on a fatal fault **without any taktora code
running afterwards** (:need:`ADR_0065`). What is missing is the process
*above* it that turns N such binaries into a deployment.

The quality goals the orchestration layer is optimised for:

.. quality-goal:: Deterministic, dependency-ordered bring-up
   :id: QG_0200
   :status: open
   :refines: FEAT_0200

   Processes shall start in a declared dependency order, and a dependent
   shall not be considered started until its provider is *ready* (not
   merely spawned). This closes the reader-before-writer race that the
   ``integrity-cross-process`` example warns about (iceoryx2 pub/sub is
   not retroactive beyond ``history_size``).

.. quality-goal:: Bounded fault detection and recovery
   :id: QG_0201
   :status: open
   :refines: FEAT_0200

   Loss of a process's liveness shall be detected within FTTI/2
   (≤ 50 ms for the automotive target) via the heartbeat of
   :need:`TSR_0010`, and the configured recovery action (restart of the
   process or its run group) shall be applied deterministically.

.. quality-goal:: Fail-safety independent of orchestrator liveness
   :id: QG_0202
   :status: open
   :refines: FEAT_0200

   The safe-state path shall not depend on the orchestrator, the crashed
   process, or any taktora code running after the fault. Outputs reach a
   safe state through the fieldbus watchdog (:need:`ADR_0065`,
   :need:`AOU_0016`) regardless of what the supervisor does next.

.. quality-goal:: Reuse-first, minimal bespoke surface
   :id: QG_0203
   :status: open
   :refines: FEAT_0200

   Where a hardened init/supervisor already provides ordering, restart,
   and watchdog semantics (systemd on Linux), the deployment shall reuse
   it rather than reimplement it. A bespoke supervisor is justified only
   by requirements the reused tool cannot meet (cross-process system
   states, safety-argument coupling).

.. quality-goal:: Cross-process integrity preserved across the lifecycle
   :id: QG_0204
   :status: open
   :refines: FEAT_0200

   Startup, restart, and state transitions shall not weaken the
   spatial-isolation invariant of :need:`TSR_0009` / :need:`TSR_0003`:
   safety-critical and QM-grade code stay in distinct OS processes
   communicating only over iceoryx2 single-writer channels
   (:need:`TSR_0007`, :need:`AOU_0008`).

2. Constraints
--------------

.. constraint:: The OS process is the unit of orchestration
   :id: CON_0200
   :status: open
   :refines: FEAT_0200

   The orchestrator shall treat a taktora process as an opaque managed
   unit. It shall not reach inside a process to schedule items; the
   executor owns in-process scheduling. There is no in-process
   supervisor and none shall be introduced.

.. constraint:: iceoryx2 shared memory is the only cross-process data plane
   :id: CON_0201
   :status: open
   :refines: FEAT_0200

   Cross-process data flow shall be exclusively iceoryx2 SHM channels
   (:need:`AOU_0008`). The orchestrator's own control plane (launch,
   health, commands) may use any transport, but shall not become a second
   data path between managed processes.

.. constraint:: taktora performs no executable authentication
   :id: CON_0202
   :status: open
   :refines: FEAT_0200

   Unlike a full execution-management daemon, taktora does not checksum or
   verify signatures of the binaries it runs. If provenance is required,
   the launcher (:need:`BB_0202`) shall verify before ``exec``; this is
   an added responsibility, not an existing capability.

.. constraint:: Linux-first realisation
   :id: CON_0203
   :status: open
   :refines: FEAT_0200

   The production realisation targets Linux (the platform of the
   ``timerfd`` absolute grid :need:`BB_0095`, ``SCHED_FIFO``
   :need:`REQ_0041`, and systemd). A QNX realisation parallels the
   platform-native supervisor model and is out of scope for the first drop.

.. constraint:: Restart policy is a host responsibility
   :id: CON_0204
   :status: open
   :refines: FEAT_0200

   Consistent with taktora's existing posture, the *decision* to restart
   a crashed process lives in the orchestration layer, never in the
   executor. The executor's contribution is to fail fast and observably
   (:need:`ADR_0065`), not to self-heal.

3. Context and scope
--------------------

.. architecture:: Orchestration system context
   :id: ARCH_0200
   :status: open
   :refines: FEAT_0200

   The orchestrator sits between the platform init/operator and a set of
   taktora processes, coupling to each process only through taktora's
   lifecycle/health contract (:need:`BB_0200`). A **diverse, independent
   Element B monitor** (per the SEooC decomposition,
   :doc:`../../safety/decomposition`) is a peer process the orchestrator
   launches but does **not** implement.

   .. mermaid::

      flowchart TB
        OP["Platform init / operator<br/>(systemd, or boot script)"]
        ORCH["Orchestrator<br/>(systemd units, or taktora-conductor)"]
        subgraph MANAGED["Managed taktora processes"]
          direction LR
          P1["SC executor process<br/>(heartbeat, admission, HealthEvent)"]
          P2["Gateway process<br/>(ConnectorGateway + stack)"]
          P3["QM executor process<br/>(telemetry / non-safety)"]
        end
        MON["Element B monitor<br/>(diverse, independent process)"]
        IOX[("iceoryx2 SHM<br/>data plane")]
        BUS[("fieldbus + drives<br/>SM-watchdog → safe state")]

        OP --> ORCH
        ORCH -->|"launch · order · restart · state"| MANAGED
        P1 -->|"heartbeat ≤ FTTI/2"| MON
        MANAGED <--> IOX
        P2 -->|"PDO"| BUS

   **In scope.** Launch and dependency-ordered bring-up; readiness
   handshake; iceoryx2 service/SHM provisioning; liveness watch and
   restart; cross-process system-state transitions; reverse-order
   shutdown; optional launch-time executable authentication.

   **Out of scope.** In-process scheduling (owned by the executor);
   hard-real-time guarantees; the safe-state mechanism itself (owned by
   the fieldbus watchdog, :need:`ADR_0065`); the *content* of the Element
   B monitor's diverse check (an integrator Assumption-of-Use); QNX
   realisation.


4. Solution strategy
--------------------

The strategy is **two-track**, chosen per deployment by whether an
existing supervisor can meet the requirements.

* **Track A — reuse systemd (default, Linux).** A taktora process is
  already ``Type=notify``/watchdog/restart-shaped, so ordinary systemd
  units express ordering, readiness, restart, and a watchdog fed by the
  heartbeat. Zero bespoke code. Chosen unless a requirement below forces
  Track B.
* **Track B — taktora-conductor (bespoke supervisor).** A small
  supervisor binary that reads a manifest in a declarative
  service / run-group / system-state / dependency /
  handshake) and drives the same taktora contract surface. Chosen when
  the deployment needs **cross-process system states** or a
  **safety-argument coupling** systemd does not express.

.. arch-decision:: Reuse an existing init/supervisor where it suffices
   :id: ADR_0200
   :status: open
   :refines: FEAT_0200
   :links: QG_0203, REQ_0050, REQ_0051, TSR_0010

   **Context.** Ordering, restart with backoff, and a liveness watchdog
   are solved problems in hardened init systems. taktora already meets
   the systemd notify/watchdog contract: ``run()`` returns cleanly on
   SIGINT/SIGTERM (:need:`REQ_0050`), a ``Stoppable`` handle stops it
   programmatically (:need:`REQ_0051`), and ``Observer::on_heartbeat``
   (:need:`TSR_0010`) is a natural ``WATCHDOG=1`` source.

   **Decision.** On Linux, the default realisation is systemd units. A
   taktora process raises ``READY=1`` **after** its admission gate
   (:need:`TSR_0011`) passes, feeds ``WATCHDOG=1`` from the heartbeat,
   and relies on ``After=``/``Requires=`` for ordering and ``Restart=``
   for recovery. No bespoke supervisor is built for this track.

   **Alternatives considered.**

   * *Always build a bespoke supervisor.* Rejected — duplicates hardened
     init behaviour and adds a single-point-of-failure to maintain, for
     no benefit on deployments without cross-process states.
   * *Bare boot script (no supervisor).* Rejected — no readiness gating,
     no restart, no watchdog; reintroduces the reader-before-writer race
     (:need:`QG_0200`).

   **Consequences.** systemd gives ordering, restart, and watchdog for
   free but expresses **no** safety argument and only crude "targets" for
   states; deployments needing cross-process system states or the SEooC
   coupling escalate to Track B.

.. arch-decision:: Model the bespoke supervisor on a declarative service model
   :id: ADR_0201
   :status: open
   :refines: FEAT_0200
   :links: FEAT_0019, ARCH_0021

   **Context.** Where Track A is insufficient, the missing concepts —
   grouping processes controlled together, sets of groups that run in a
   given vehicle/machine mode, inter-process start dependencies — are
   exactly the classic process-supervisor model (Run Group, System State, dependency,
   handshake). taktora's own mode/state-machine feature
   (:need:`FEAT_0019`) is in-process only and does not span processes.

   **Decision.** ``taktora-conductor`` adopts a process-supervisor domain model at
   process granularity: **Service** (one managed process), **Run Group**
   (services controlled together, started in dependency order, stopped in
   reverse), **System State** (a set of run groups), **Dependency**
   (``needs``/``state`` between services), **Handshake** (readiness
   gate). This is a control-plane analogue of the connector's
   separate-process deployment (:need:`ARCH_0021`).

   **Alternatives considered.**

   * *Invent a fresh vocabulary.* Rejected — discards a battle-tested,
     safety-oriented model and its documented semantics for no gain.
   * *Extend the in-process mode machine (FEAT_0019) across processes.*
     Rejected — couples the executor core to a supervisor concern,
     violating :need:`CON_0200`.

   **Consequences.** Integrators familiar with PLC/init supervisors map their mental model
   directly. The conductor stays a thin control plane; the data plane is
   untouched (:need:`CON_0201`).

.. arch-decision:: Readiness is first heartbeat / HealthEvent::Up, not spawn
   :id: ADR_0202
   :status: open
   :refines: FEAT_0200
   :links: TSR_0010, TSR_0011, QG_0200

   **Context.** "Process spawned" is not "process ready". A dependent
   started against a not-yet-subscribed provider silently loses the first
   publications (the example's late-joiner hazard). A robust handshake
   needs an in-band readiness signal.

   **Decision.** The readiness predicate for a service is its **first
   heartbeat tick or first ``HealthEvent::Up``** on the health channel —
   emitted only after the admission gate (:need:`TSR_0011`) has admitted
   the item set. The orchestrator blocks a dependent's launch on the
   provider's readiness (bounded by a per-service handshake timeout);
   timeout is a start fault, handled by the run group's policy.

   **Alternatives considered.**

   * *Fixed sleep between launches.* Rejected — races under load, wastes
     time when fast; unrepeatable.
   * *iceoryx2 service existence as readiness.* Rejected — the service
     can exist before the subscriber is attached; existence ≠ subscribed.

   **Consequences.** Readiness reuses the exact channel the watchdog uses
   (:need:`TSR_0010`), so no new taktora surface is required for Track A
   or Track B.

.. arch-decision:: Safety supervision needs a diverse Element B, not the conductor
   :id: ADR_0203
   :status: open
   :refines: FEAT_0200
   :links: TSR_0010, ADR_0065, AOU_0016

   **Context.** The SEooC concept claims ASIL D by decomposition
   (``ASIL D = ASIL B(D) + ASIL B(D)``): taktora is Element A; the
   integrator supplies a **diverse, independent monitor** (Element B).
   Independence is an Assumption-of-Use not closed by taktora
   (:doc:`../../safety/decomposition`).

   **Decision.** The orchestrator **launches** the Element B monitor as a
   separate process and wires the heartbeat channel to it, but does
   **not** implement or subsume it. The safe-state reaction remains the
   fieldbus watchdog (:need:`ADR_0065`, :need:`AOU_0016`), so it holds
   even if the orchestrator itself dies (:need:`QG_0202`). A conductor
   that also acted as the monitor would defeat the independence argument.

   **Alternatives considered.**

   * *Conductor is the safety monitor.* Rejected — a common-cause
     dependency between supervision and the supervised control plane;
     breaks the decomposition independence claim.
   * *No monitor, rely only on the SM-watchdog.* Rejected — the
     SM-watchdog covers output freshness but not arbitrary logical
     faults the diverse monitor is meant to catch.

   **Consequences.** The safety story is a *deployment* property (three+
   processes: control, monitor, fieldbus-enforced safe state), not a
   supervisor feature.

5. Building block view
----------------------

The orchestration layer decomposes into a taktora-side **contract
surface** (already present) and a supervisor-side set of blocks (Track B;
Track A maps them onto systemd primitives instead).

.. building-block:: taktora orchestration contract surface
   :id: BB_0200
   :status: implemented
   :implements: REQ_1201
   :refines: FEAT_0200

   The set of *existing* taktora hooks an orchestrator couples to. This
   is the only taktora-side surface; it needs no new code.

   * **Clean stop** — ``run()`` returns on SIGINT/SIGTERM
     (:need:`REQ_0050`); clonable ``Stoppable::stop()`` wakes the WaitSet
     in bounded time (:need:`REQ_0051`, ``BB_0035``).
   * **Readiness / liveness** — ``ExecutorBuilder::heartbeat`` +
     ``Observer::on_heartbeat`` emit ``HeartbeatTick { seq, at_nanos }``
     at ≤ FTTI/2; the connector-host ``HeartbeatHealthBridge`` forwards
     it onto the ``HealthEvent`` channel (:need:`TSR_0010`).
   * **Cold-start gate** — ``ExecutorBuilder::admission_check`` runs
     verify → admit → ``RUNNING`` before any dispatch; rejection yields
     ``AdmissionRejected`` with nothing dispatched (:need:`TSR_0011`).
   * **Peer health** — four-state ``ConnectorHealth``
     (Up/Connecting/Degraded/Down), bounded latency (:need:`TSR_0006`).
   * **Fail-fast** — framework-invariant violation ⇒ fatal handler ⇒
     ``process::abort`` ⇒ fieldbus SM-watchdog drives outputs safe
     (:need:`ADR_0065`).

.. building-block:: Conductor manifest model
   :id: BB_0201
   :status: open
   :implements: REQ_1200, REQ_1204, REQ_1205
   :refines: ADR_0201

   The declarative model parsed by ``taktora-conductor``: ``Service``,
   ``RunGroup``, ``SystemState``, ``Dependency``, and per-service
   scheduling/handshake/restart attributes. Illustrative manifest:

   .. code-block:: toml

      [[service]]                     # one managed process
      id = "nc_hotpath"
      exe = "/opt/app/nc_hotpath"
      integrity = "SafetyCritical"
      sched = { policy = "FIFO", prio = 80, affinity = [3] }  # REQ_0041 / REQ_0040
      heartbeat_timeout_ms = 20       # readiness + watchdog bound (TSR_0010)
      restart = "on-failure"          # CON_0204
      verify_signature = true         # optional; CON_0202

      [[service]]
      id = "ecat_gateway"
      exe = "/opt/app/ecat_gateway"
      integrity = "SafetyCritical"
      restart = "on-failure"

      [[run_group]]                   # controlled together
      id = "motion"
      members = ["ecat_gateway", "nc_hotpath"]
      deps = [{ from = "nc_hotpath", needs = "ecat_gateway", state = "Up" }]

      [[system_state]]                # a machine/vehicle mode
      id = "drive"
      run_groups = ["motion", "telemetry"]

.. building-block:: Launcher
   :id: BB_0202
   :status: open
   :implements: REQ_1200, REQ_1207
   :refines: ADR_0201

   Spawns each service via ``std::process`` with its declared scheduling
   attributes (``SCHED_FIFO`` prio needs ``CAP_SYS_NICE``,
   :need:`REQ_0041`; core affinity :need:`REQ_0040`), optionally
   verifying the binary's signature first (:need:`CON_0202`). Owns the
   child handle for signal delivery and exit-code observation.

.. building-block:: SHM provisioner
   :id: BB_0203
   :status: open
   :implements: REQ_1202
   :refines: ADR_0201

   Pre-creates the iceoryx2 services a run group needs with pinned QoS
   (single-writer for safety-critical channels, :need:`TSR_0007`;
   ``history_size`` for tolerable late-join) **before** dependents launch,
   and reaps stale ``/dev/shm`` services on group teardown. The
   provisioning analogue of a central shared-memory broker setup.

.. building-block:: Health aggregator and restart engine
   :id: BB_0204
   :status: open
   :implements: REQ_1201, REQ_1203
   :refines: ADR_0202

   Subscribes to every service's heartbeat + ``HealthEvent`` channel;
   treats a missed heartbeat within ``heartbeat_timeout_ms`` (≤ FTTI/2)
   or a child abort exit as a fault; applies the service's / run group's
   restart policy; and surfaces aggregated state for system-state
   decisions. Never runs safety logic — that is the Element B monitor
   (:need:`ADR_0203`).

.. building-block:: systemd integration bridge
   :id: BB_0205
   :status: open
   :implements: REQ_1201
   :refines: ADR_0200

   The Track-A alternative to :need:`BB_0204`: a thin adapter that raises
   ``sd_notify(READY=1)`` after admission and ``sd_notify(WATCHDOG=1)``
   from ``on_heartbeat``, so systemd's ``WatchdogSec`` + ``Restart=``
   provide detection and recovery. Sample unit:

   .. code-block:: ini

      # nc-hotpath.service
      [Service]
      Type=notify
      ExecStart=/opt/app/nc_hotpath
      WatchdogSec=100ms
      Restart=on-failure
      CPUAffinity=3
      Requires=ecat-gateway.service
      After=ecat-gateway.service

Requirements realised
~~~~~~~~~~~~~~~~~~~~~~~

.. req:: Dependency-ordered startup
   :id: REQ_1200
   :status: open
   :satisfies: FEAT_0200
   :links: QG_0200, ARCH_0021

   The orchestrator shall start services in an order consistent with the
   declared dependency graph, and shall reject a manifest whose
   dependency graph contains a cycle.

.. req:: Readiness handshake before dependents
   :id: REQ_1201
   :status: open
   :satisfies: FEAT_0200
   :links: ADR_0202, TSR_0010, TSR_0011

   A service with unmet dependencies shall not be launched until each
   provider signals readiness (first heartbeat or ``HealthEvent::Up``),
   bounded by a per-service handshake timeout after which the start is a
   fault.

.. req:: Shared-memory provisioning ordering
   :id: REQ_1202
   :status: open
   :satisfies: FEAT_0200
   :links: TSR_0007, AOU_0008

   The orchestrator shall ensure the iceoryx2 services a run group
   requires exist with their declared QoS before any consumer of those
   services is launched.

.. req:: Health aggregation and recovery
   :id: REQ_1203
   :status: open
   :satisfies: FEAT_0200
   :links: TSR_0010, CON_0204

   The orchestrator shall detect service liveness loss within FTTI/2 and
   apply the declared restart policy for the affected service or run
   group.

.. req:: Cross-process system-state transition
   :id: REQ_1204
   :status: open
   :satisfies: FEAT_0200
   :links: FEAT_0019

   On a system-state change request, the orchestrator shall start the run
   groups of the target state that are not running and stop those not
   associated with it, leaving shared run groups already running
   untouched.

.. req:: Reverse-order shutdown
   :id: REQ_1205
   :status: open
   :satisfies: FEAT_0200
   :links: REQ_0050, REQ_0051

   The orchestrator shall stop a run group's services in the reverse of
   their start order, delivering SIGTERM (or ``Stoppable::stop``) and
   escalating to SIGKILL only after a bounded grace period.

.. req:: Independent Element B monitor process
   :id: REQ_1206
   :status: open
   :satisfies: FEAT_0200
   :links: ADR_0203, TSR_0010

   For a safety deployment, the orchestrator shall launch the integrator's
   diverse monitor as a distinct process wired to the safety-critical
   services' heartbeat channel, and shall not itself perform the diverse
   safety check.

.. req:: Optional launch-time executable authentication
   :id: REQ_1207
   :status: open
   :satisfies: FEAT_0200
   :links: CON_0202

   Where configured, the launcher shall verify a binary's integrity
   (checksum + signature) before executing it, refusing to start the
   service on failure.

6. Runtime view
---------------

**Scenario R1 — dependency-ordered cold start (motion run group).**

.. mermaid::

   sequenceDiagram
       participant C as Orchestrator
       participant S as SHM provisioner
       participant GW as ecat_gateway
       participant NC as nc_hotpath
       participant MON as Element B monitor
       C->>S: provision iceoryx2 services (QoS pinned)
       C->>GW: launch
       GW-->>C: HealthEvent::Up (ready)
       Note over C: dep {nc_hotpath needs ecat_gateway=Up} satisfied
       C->>NC: launch
       NC->>NC: admission_check → RUNNING
       NC-->>C: first heartbeat (ready)
       C->>MON: launch, wire NC heartbeat
       Note over C,MON: run group "motion" is up

**Scenario R2 — heartbeat miss → restart.** The aggregator
(:need:`BB_0204`) sees no ``nc_hotpath`` heartbeat within
``heartbeat_timeout_ms``; it applies ``restart = on-failure``: stop the
run group in reverse order (:need:`REQ_1205`), then re-run R1 for the
group. Meanwhile the SM-watchdog has already held/safed outputs — the
restart is not on the safety path.

**Scenario R3 — crash → fail-safe → restart.** A framework-invariant
violation in ``nc_hotpath`` triggers the fatal handler and
``process::abort`` (:need:`ADR_0065`); no destructors run, the master
stops emitting PDO frames, each output slave's SM-watchdog expires within
≤ FTTI/2 and applies safe-state values. The child's abort exit is
observed by the launcher (:need:`BB_0202`); the restart engine
(:need:`BB_0204`) recovers per policy. **Safe-state reached with zero
dependency on the orchestrator (:need:`QG_0202`).**

**Scenario R4 — system-state transition drive → parking.** The
orchestrator diffs the target state's run groups against the running set
(:need:`REQ_1204`): starts ``parking``-only groups, stops
``drive``-only groups in reverse order, leaves groups common to both
running.

**Scenario R5 — graceful shutdown.** SIGTERM to the orchestrator ⇒ stop
every run group in reverse start order (:need:`REQ_1205`); each taktora
process returns cleanly from ``run()`` (:need:`REQ_0050`); the SHM
provisioner reaps services last.

7. Deployment view
------------------

.. architecture:: Track A — systemd-supervised deployment
   :id: ARCH_0201
   :status: open
   :refines: ADR_0200

   Each taktora process is a ``Type=notify`` unit; ordering is
   ``After=``/``Requires=``, recovery is ``Restart=``, liveness is
   ``WatchdogSec`` fed from the heartbeat via :need:`BB_0205`. System
   states are systemd targets. No bespoke supervisor binary.

.. architecture:: Track B — taktora-conductor deployment
   :id: ARCH_0202
   :status: open
   :refines: ADR_0201

   A ``taktora-conductor`` process reads the manifest (:need:`BB_0201`)
   and owns launch (:need:`BB_0202`), SHM provisioning (:need:`BB_0203`),
   and health/restart (:need:`BB_0204`). Chosen for cross-process system
   states or the SEooC coupling. Topology for the motion example:

   .. mermaid::

      flowchart TB
        CON["taktora-conductor"]
        subgraph SCP["SC process — nc_hotpath (FIFO 80, core 3)"]
          NC["Executor: cyclic NC @1 ms (motion + cia402)"]
        end
        subgraph GWP["gateway process — ecat_gateway"]
          EC["ConnectorGateway + ethercrab"]
        end
        subgraph QMP["QM process — telemetry"]
          TE["Executor: NDJSON export (off-RT)"]
        end
        MON["Element B monitor (separate process)"]
        IOX[("iceoryx2 SHM")]
        BUS[("EtherCAT + CiA 402 drives")]
        CON -->|"provision · launch · order · restart"| SCP & GWP & QMP & MON
        NC <--> IOX <--> EC
        NC -->|"AxisStatus"| IOX --> TE
        NC -->|"heartbeat ≤ FTTI/2"| MON
        EC -->|"PDO"| BUS

Mapping of orchestration concerns to each track:

.. list-table::
   :header-rows: 1
   :widths: 30 35 35

   * - Concern
     - Track A (systemd)
     - Track B (conductor)
   * - Ordering
     - ``After=`` / ``Requires=``
     - dependency graph (:need:`REQ_1200`)
   * - Readiness
     - ``Type=notify`` ``READY=1``
     - first heartbeat / Up (:need:`REQ_1201`)
   * - Liveness watchdog
     - ``WatchdogSec`` + ``WATCHDOG=1``
     - health aggregator (:need:`BB_0204`)
   * - Restart
     - ``Restart=`` / ``RestartSec=``
     - restart engine (:need:`REQ_1203`)
   * - System states
     - systemd targets
     - ``SystemState`` (:need:`REQ_1204`)
   * - SHM provisioning
     - ``oneshot`` provisioning unit
     - SHM provisioner (:need:`BB_0203`)
   * - Element B monitor
     - separate unit
     - separate service (:need:`REQ_1206`)

8. Crosscutting concepts
------------------------

**Integrity preservation across restart.** A restarted service is
re-launched with the same ``IntegrityLevel`` pin and the same
single-writer channel capabilities (:need:`TSR_0009`, :need:`TSR_0007`);
the admission gate (:need:`TSR_0011`) re-verifies the isolation context
on every cold start, so a restart cannot silently downgrade isolation
(:need:`QG_0204`).

**Shared-memory lifecycle.** iceoryx2 services are persistent
``/dev/shm`` resources; the first opener creates, later openers attach,
and ``open_or_create`` reaps stale services. The provisioner
(:need:`BB_0203`) makes creation explicit and ordered so QoS is pinned
by the owner, not raced by whichever consumer starts first.

**Time and watchdog budget.** All liveness bounds are expressed against
FTTI/2 (≤ 50 ms automotive): heartbeat period (:need:`TSR_0010`), health
transition latency (:need:`TSR_0006`), and the SM-watchdog timeout
(:need:`AOU_0016`). The orchestrator's detection timeout must be ≥ the
heartbeat period and ≤ FTTI/2.

**Observability.** The orchestrator consumes the same health/heartbeat
channels used for supervision; per-process logs flow through the existing
logging facade / DLT backend. No separate orchestration telemetry plane
is introduced (:need:`CON_0201`).

**Security.** taktora authenticates nothing (:need:`CON_0202`); if the
threat model requires it, :need:`REQ_1207` moves signature verification
into the launcher — the one place a process boundary is crossed.

**QNX parallel.** On QNX the same model maps onto the platform's own
launch/security-policy tooling; the manifest and
readiness/handshake semantics are portable, the launcher and scheduling
primitives are not.

9. Architecture decisions
-------------------------

.. needtable::
   :filter: id in ["ADR_0200", "ADR_0201", "ADR_0202", "ADR_0203"]
   :columns: id, title, status, refines

10. Quality requirements
------------------------

Quality scenarios refining the goals of §1 into measurable acceptance
criteria:

.. list-table::
   :header-rows: 1
   :widths: 20 50 30

   * - Goal
     - Scenario
     - Measure
   * - :need:`QG_0200`
     - A dependent launched before its provider is ready
     - 0 lost first-cycle publications across N cold starts
   * - :need:`QG_0201`
     - A service stops emitting heartbeats
     - fault raised ≤ FTTI/2 (≤ 50 ms)
   * - :need:`QG_0202`
     - Orchestrator killed at the moment a service crashes
     - outputs still reach safe state via SM-watchdog
   * - :need:`QG_0203`
     - Deployment without cross-process states
     - realised with systemd units only, no bespoke code
   * - :need:`QG_0204`
     - Service restarted
     - re-admitted with identical integrity + channel caps

11. Risks and technical debt
----------------------------

.. risk:: iceoryx2 service leakage across restart
   :id: RISK_0200
   :status: open
   :links: BB_0203, CON_0201

   A crashed process may leave a ``/dev/shm`` service whose QoS conflicts
   with the restarted instance ("service already exists with incompatible
   QoS"). Mitigation: the provisioner owns create/reap; restart reaps
   before re-create.

.. risk:: Readiness false-negative under load
   :id: RISK_0201
   :status: open
   :links: ADR_0202, TSR_0010

   Heartbeat-as-readiness can time out if a provider is CPU-starved at
   start. Mitigation: separate (longer) handshake timeout from the
   (tighter) steady-state watchdog timeout; pin/prioritise SC services
   (:need:`REQ_0041`).

.. risk:: No executable authentication by default
   :id: RISK_0202
   :status: open
   :links: CON_0202, REQ_1207

   taktora runs unverified binaries by default. Until :need:`REQ_1207`
   is implemented, provenance depends entirely on filesystem/OS controls.

.. risk:: systemd unavailable on the target
   :id: RISK_0203
   :status: open
   :links: ADR_0200, CON_0203

   Track A assumes systemd; QNX and minimal-init targets force Track B or
   a platform-native supervisor, widening the bespoke surface.

.. risk:: Watchdog false-positive restart storm
   :id: RISK_0204
   :status: open
   :links: BB_0204, QG_0201

   An aggressive detection timeout under transient load can restart a
   healthy service repeatedly. Mitigation: bounded backoff; detection
   timeout ≥ heartbeat period; correlate with ``HealthEvent`` before
   acting.

.. risk:: SHM pool sizing is manual
   :id: RISK_0205
   :status: open
   :links: BB_0203, ARCH_0021

   Separate-process deployments require the peer's SHM pool to be sized
   ahead of time; under-sizing surfaces only at runtime. Mitigation:
   derive sizes from the manifest's channel specs.

.. risk:: Conductor as common-cause dependency
   :id: RISK_0206
   :status: open
   :links: ADR_0203, QG_0202

   If future work lets the conductor take safety-relevant reactions, it
   risks becoming a common-cause dependency with the control plane.
   Mitigation: keep safety reactions in the fieldbus watchdog and the
   diverse Element B monitor only.

12. Glossary
------------

.. term:: Orchestrator
   :id: GLOSS_0200
   :status: open

   The process (systemd, or ``taktora-conductor``) that launches,
   sequences, supervises, and recovers managed taktora processes.

.. term:: Conductor
   :id: GLOSS_0201
   :status: open

   ``taktora-conductor`` — the bespoke Track-B supervisor built on the
   process-supervisor domain model.

.. term:: Service (orchestration)
   :id: GLOSS_0202
   :status: open

   One managed taktora OS process, the unit of orchestration
   (:need:`CON_0200`).

.. term:: Run Group
   :id: GLOSS_0203
   :status: open

   A set of services controlled together — started in dependency order,
   stopped in reverse.

.. term:: System State
   :id: GLOSS_0204
   :status: open

   A set of run groups that should run together in a given machine/vehicle
   mode.

.. term:: Handshake / readiness
   :id: GLOSS_0205
   :status: open

   The gate a provider must pass before dependents launch: its first
   heartbeat or ``HealthEvent::Up`` (:need:`ADR_0202`).

.. term:: Element B monitor
   :id: GLOSS_0206
   :status: open

   The diverse, independent monitor process supplied by the integrator to
   close the ASIL-D-by-decomposition independence argument
   (:doc:`../../safety/decomposition`).

.. term:: FTTI
   :id: GLOSS_0207
   :status: open

   Fault-Tolerant Time Interval. All liveness and watchdog bounds here
   are expressed against FTTI/2 (≤ 50 ms automotive).

.. term:: Orchestratee
   :id: GLOSS_0208
   :status: open

   A taktora process viewed as a managed unit — designed to be launched,
   watched, and recovered by an orchestrator, exposing only the contract
   surface of :need:`BB_0200`.
