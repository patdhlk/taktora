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

This component is organised under the umbrella feature below; each
capability cluster has its own page (see the toctree). The
:ref:`trajectory core <trajectory-core>` is the first cluster.

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

Requirements at a glance
------------------------

.. needtable::
   :columns: id, title, status, satisfies
   :show_filters:
   :filter: "FEAT_0090" in satisfies or "FEAT_0091" in satisfies

.. toctree::
   :maxdepth: 2

   trajectory-core
