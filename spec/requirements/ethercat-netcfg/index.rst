EtherCAT network-config codegen
===============================

This page captures the requirements for the **EtherCAT network-config
codegen toolchain**: a layered set of build-host crates that translate
an integrator-authored ``network.yaml`` describing one bus's topology
and application channel wiring into the ``&'static`` tables
(``SubDeviceMap``, ``EthercatRouting``, channel-name constants) that
``taktora-connector-ethercat`` consumers hand-write today.

It is the network/topology peer of the device-internal codegen
toolchains:

* :need:`FEAT_0050` (``ethercat-esi``) translates a vendor ESI XML file
  into a typed driver for **one device's** object dictionary / PDO
  catalog.
* :need:`FEAT_0060` (``canopen-eds``) does the same for CANopen EDS.
* **This umbrella** (:need:`FEAT_0080`) describes how *this* integrator
  wires *these* devices into one bus's process image, and which executor
  channels bind to which process-data slices.

ESI/EDS describe what a device *can* do (vendor-supplied); this
toolchain describes how the integrator wires devices together
(integrator-authored). They compose: a device entry in the YAML may
reference an ESI file to inherit its PDO catalog and identity.

The design rationale and the full grilling trail live in
``docs/superpowers/specs/2026-05-31-ethercat-netcfg-codegen-design.md``.
The decomposition mirrors the convention established in
:doc:`../device-codegen/index`: a top-level umbrella feature, capability-cluster
sub-features that ``:satisfies:`` it, and concrete shall-clauses under
each cluster.

This round covers EtherCAT only and one bus per file. Multi-bus
documents / distribution over multiple files are explicitly deferred
(see :need:`ADR_0096`).

Top-level umbrella
------------------

.. feat:: EtherCAT network-config codegen toolchain
   :id: FEAT_0080
   :status: open

   A layered set of build-host Rust crates that consume an
   integrator-authored ``network.yaml`` (one bus per file) and emit, at
   build time, the ``&'static`` bus tables a ``taktora-connector-ethercat``
   application consumes. The toolchain is organised as four layers that
   depend only leftwards:

   1. **Parse layer** — ``ethercat-netcfg``: YAML → typed IR. Depends on
      ``ethercat-esi`` and ``fieldbus-od-core`` to resolve ESI references;
      no dependency on the connector runtime.
   2. **Codegen layer** — ``ethercat-netcfg-codegen``: IR →
      ``TokenStream`` (``prettyplease`` formatting).
   3. **Tooling layer** — ``ethercat-netcfg-build`` (build.rs glue) and
      ``ethercat-netcfg-cli`` (``netcfg expand`` / ``netcfg fetch``).

   The generated code names ``taktora-connector-ethercat`` types
   (``SubDeviceMap``, ``PdoEntry``, ``EthercatRouting``) textually; the
   codegen crates never link the connector runtime, and no change to
   :need:`FEAT_0041`'s runtime contracts is required (mirror of the
   ESI toolchain's no-runtime-modification rule).

----

Capability clusters
-------------------

The umbrella decomposes into five capability clusters. Each cluster is a
sub-feature ``:satisfies:`` :need:`FEAT_0080`, with concrete
shall-clauses underneath.

Requirements at a glance
------------------------

.. needtable::
   :types: req, feat
   :columns: id, title, status, satisfies
   :filter: "FEAT_0080" in satisfies or "FEAT_0081" in satisfies or "FEAT_0082" in satisfies or "FEAT_0083" in satisfies or "FEAT_0084" in satisfies or "FEAT_0085" in satisfies or id in ["FEAT_0080", "FEAT_0081", "FEAT_0082", "FEAT_0083", "FEAT_0084", "FEAT_0085"]
   :show_filters:

.. toctree::
   :maxdepth: 2

   parser-ir
   codegen
   build-glue
   cli-vendoring
   validation
