Motion — real-time trajectory generation for CSP drives
=======================================================

Requirements for taktora's motion-control stack. The stack is split into
a pure algorithmic core (``taktora-motion-core`` — ``no_std``,
allocation-free trajectory generation) and the executor/connector glue
(``taktora-motion``, deferred). The deployment model is a **setpoint
generator** feeding CiA 402 servo drives in Cyclic Synchronous Position
(CSP) mode: the core produces the commanded position each cycle; the
drive closes its own velocity and current loops.

The cyclic control loop runs in **one real-time process per EtherCAT
network** (one network = one master = one cycle); axes scale inside a
single ``AxisGroup``, not across processes. iceoryx2 carries commands,
telemetry, and the diverse safety monitor — never the cyclic setpoint.

Top-level umbrella
------------------

.. feat:: Real-time motion control
   :id: FEAT_0090
   :status: open

   A soft-real-time motion-control capability layered on the taktora
   runtime: per-cycle, bounded, allocation-free setpoint generation for
   coordinated multi-axis machines (point-to-point profiles, electronic
   gearing, camming, flying saw), delivered to CiA 402 drives in CSP
   mode.

   The umbrella satisfies two forces that collide in a naive motion
   kernel:

   1. **Motion wants expressive, stateful generators** — profiles,
      cam tables, synchronized coupling — classically modelled with
      boxed trait objects and growable tables.
   2. **The taktora runtime forbids both on the hot path.**
      :need:`REQ_0060` mandates zero heap allocation in steady-state
      dispatch and :need:`FEAT_0017` mandates bounded-time dispatch.

   The resolution is a monomorphized generator (an ``enum``, no
   ``Box<dyn>``, no vtable) over pre-provisioned fixed-capacity state,
   evaluated as bounded polynomial arithmetic. The umbrella decomposes
   into capability clusters, the first of which is the trajectory core
   below.

----

Capability clusters
-------------------

.. feat:: Allocation-free trajectory core
   :id: FEAT_0091
   :status: open
   :satisfies: FEAT_0090

   ``taktora-motion-core`` — the pure algorithmic layer. Commanded axis
   setpoints are computed as bounded, allocation-free, panic-free
   functions of ``(dt, master)``. The crate owns no I/O, no threads, and
   no shared mutable state, and carries no dependency on the executor or
   iceoryx2, so it is host-testable with no runtime and its temporal and
   information-exchange independence (for the diverse-monitor safety
   argument) can be reasoned about without the soft-RT machinery.

   The ``Motion`` generators — ``Idle``, ``Velocity`` (also the virtual
   master), ``Trapezoid``, jerk-limited ``SCurve``, ``Gear`` (electronic
   gearing), ``FlyingSaw`` (quintic catch-up / synchronous / return), and
   ``Cam`` (``&'static`` quintic table) — are ticked through a
   fixed-capacity ``AxisGroup`` in a build-time topological order (masters
   before slaves) so coupling is same-cycle coherent. Superimposed
   corrective motion (PLCopen ``MC_MoveSuperimposed``) is realised as an
   additive jerk-limited overlay on the ``Axis`` itself rather than a
   generator arm — a ``Motion`` cannot hold a ``Motion`` without heap
   indirection. The ``SCurve`` profile is cross-checked against the Ruckig
   time-optimal oracle; on-the-fly retargeting from a non-zero state
   (Ruckig *online*) remains a deferred follow-on.
