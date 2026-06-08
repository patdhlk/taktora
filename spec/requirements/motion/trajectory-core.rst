.. _trajectory-core:

Allocation-free trajectory core
===============================

The first capability cluster of the motion umbrella — the pure
algorithmic layer ``taktora-motion-core``.

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
