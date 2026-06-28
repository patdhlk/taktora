Requirements
============

System-level requirements. The spec is organised under three peer
top-level features:

* :need:`FEAT_0010` "PLC runtime heart on iceoryx2" — taktora-executor
  framed as the runtime heart of a soft-real-time PLC. See
  :doc:`plc-runtime/index`.
* :need:`FEAT_0030` "Connector framework" — the general-purpose framework
  for bridging taktora-executor applications to external protocols. See
  :doc:`connector/index`.
* :need:`FEAT_0040` "Bounded global allocator" — workspace
  infrastructure providing a static, pre-allocated, fixed-block
  ``#[global_allocator]`` for taktora binaries that require
  compile-time guarantees on memory usage. See :doc:`bounded-alloc`.
* :need:`FEAT_0050` "Device-driver codegen toolchain" — build-time
  layered crates that translate EtherCAT ESI XML into strongly-typed
  Rust device drivers, consumed by ``taktora-connector-ethercat`` and
  any other ethercrab user. See :doc:`device-codegen/index`.
* :need:`FEAT_0060` "CANopen device-driver codegen toolchain" —
  build-time layered crates that translate CANopen EDS (CiA 306)
  files into strongly-typed Rust device drivers, with a shared
  ``fieldbus-od-core`` OD IR co-owned by the EtherCAT toolchain.
  See :doc:`canopen-codegen/index`.
* :need:`FEAT_0070` "Shared logging base library" — a workspace-wide
  logging facade (``taktora-log``) with a default AUTOSAR DLT
  backend (``taktora-log-dlt``) and a clean swap path for
  ``log4rs`` / ``env_logger`` / bespoke loggers. See :doc:`logging/index`.
* :need:`FEAT_0080` "EtherCAT network-config codegen toolchain" —
  build-time layered crates that translate an integrator-authored
  ``network.yaml`` (bus topology + channel wiring) into the
  ``&'static`` bus tables ``taktora-connector-ethercat`` consumers
  hand-write today, composing on top of the ESI device toolchain.
  See :doc:`ethercat-netcfg/index`.
* :need:`FEAT_0090` "Real-time motion control" — soft-real-time,
  allocation-free trajectory generation (profiles, electronic gearing,
  camming, flying saw) feeding CiA 402 drives in CSP mode, layered on
  the taktora runtime. See :doc:`motion/index`.
* :need:`FEAT_0100` "Runtime diagnostics (SOVD-aligned)" — a clean-room
  Rust take on the ros2_medkit diagnostic contract: a SOVD entity tree +
  DTC/fault model served over a drop-in-compatible REST surface, sourced
  from taktora's runtime through off-control-path hooks. See
  :doc:`medkit/index`.

Each ``req`` directive ``:satisfies:`` one ``feat`` parent; each
capability-cluster ``feat`` ``:satisfies:`` its top-level umbrella feature.

.. toctree::
   :maxdepth: 2

   plc-runtime/index
   connector/index
   bounded-alloc
   device-codegen/index
   canopen-codegen/index
   logging/index
   ethercat-netcfg/index
   motion/index
   medkit/index

Requirements at a glance
------------------------

.. needtable::
   :types: req
   :columns: id, title, status, satisfies
   :show_filters:
