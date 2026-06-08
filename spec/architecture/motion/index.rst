Motion — trajectory core architecture
=====================================

Detailed design for taktora's motion-control stack (:need:`FEAT_0090`).
This section collects the architecture decisions and building blocks for
``taktora-motion-core`` and, later, the executor/connector glue
``taktora-motion``. Each capability cluster has its own page (see the
toctree); the structural diagram below shows where the trajectory core
sits.

Structural overview
-------------------

The motion stack is a **setpoint generator** layered on the taktora
runtime: the pure ``no_std`` core computes the commanded position each
cycle in ``f64`` engineering units, the glue layer converts those units
to integer encoder increments at the drive boundary, and CiA 402 drives
close their own velocity and current loops in Cyclic Synchronous Position
mode.

.. mermaid::

   graph TD
       RT["taktora runtime<br/>(cyclic RT process, one per EtherCAT network)"]
       CORE["taktora-motion-core<br/>(no_std, allocation-free)<br/>AxisGroup setpoint generation → f64 commanded position"]
       GLUE["taktora-motion (glue)<br/>units → encoder increments<br/>(integer, rounds once)"]
       DRIVE["CiA 402 drives — CSP mode<br/>(close velocity + current loops)"]

       RT --> CORE
       CORE -->|"commanded position (f64 user units)"| GLUE
       GLUE -->|"target position (encoder increments)"| DRIVE

.. toctree::
   :maxdepth: 2

   trajectory-core
