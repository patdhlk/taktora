Device-driver codegen
=====================

This chapter captures the requirements for the **device-driver codegen
toolchain**: a layered set of crates that translates EtherCAT ESI XML
device descriptions into strongly-typed Rust driver modules at build
time, with zero runtime XML parsing and no dependency on the
``taktora-connector-ethercat`` runtime.

The decomposition mirrors the convention established in
:doc:`../connector/index` and :doc:`../plc-runtime/index`:

* **Top-level umbrella feature** — :need:`FEAT_0050` — peer to
  :need:`FEAT_0010` (PLC runtime heart), :need:`FEAT_0030` (Connector
  framework), and :need:`FEAT_0040` (Bounded global allocator). The
  codegen toolchain is a build-time concern orthogonal to the runtime
  connector framework; it is not bound to taktora-executor or
  taktora-connector and could be consumed by any ethercrab user.
* **Capability-cluster sub-features** — one per crate-layer concern,
  each ``:satisfies:`` :need:`FEAT_0050`.
* **Requirements** — concrete shall-clauses that ``:satisfies:`` a
  capability-cluster feature.

This round covers EtherCAT only (ESI XML → typed driver structs).
CANopen / EDS support is explicitly out of scope; the architecture
preserves the option to extract a shared object-dictionary IR later
(see :need:`ADR_0073`).

The umbrella decomposes into seven capability clusters, each on its own
page (see the toctree): the ESI parser (:need:`FEAT_0051`), the IR and
codegen backend trait (:need:`FEAT_0052`), the ethercrab backend
(:need:`FEAT_0053`), the runtime trait surface (:need:`FEAT_0054`), the
build helper (:need:`FEAT_0055`), CLI inspection (:need:`FEAT_0056`),
and EEPROM diff verification (:need:`FEAT_0057`). The deliberately
rejected anti-goals and the umbrella-level traceability tables live on
:doc:`anti-goals`.

Top-level umbrella
------------------

.. feat:: Device-driver codegen toolchain
   :id: FEAT_0050
   :status: open

   A layered set of Rust crates that consumes EtherCAT Slave
   Information (ESI) XML files and emits strongly-typed driver
   modules at build time. The toolchain is organised as four layers
   that depend only leftwards:

   1. **Parse layer** — ``ethercat-esi``: XML → typed IR, ``no_std``
      + ``alloc``. No knowledge of codegen or ethercrab.
   2. **Codegen layer** — ``ethercat-esi-codegen`` (IR →
      ``TokenStream`` via a ``CodegenBackend`` trait) plus
      ``ethercat-esi-codegen-ethercrab`` (the one concrete backend
      shipped in this round).
   3. **Tooling layer** — ``ethercat-esi-build`` (build.rs glue),
      ``ethercat-esi-cli`` (``cargo esi expand`` / ``cargo esi list``
      one-shot tools), and ``ethercat-esi-verify`` (diff ESI XML
      against captured SII EEPROM ``.bin`` dumps).
   4. **Runtime trait crate** — ``ethercat-esi-rt``: the
      ``EsiDevice`` / ``EsiConfigurable`` traits the generated
      drivers implement.

   The ``taktora-connector-ethercat`` crate (see :need:`FEAT_0041`) is
   not part of this toolchain. It sits one layer above as a thin
   adapter that maps any ``EsiDevice`` into the
   ``ethercat_hal::EthercatDevice`` trait it already consumes. No
   change to :need:`FEAT_0041`'s runtime contracts is required by
   this spec.

Requirements at a glance
------------------------

.. needtable::
   :columns: id, title, status, satisfies
   :show_filters:
   :filter: "FEAT_0050" in satisfies or "FEAT_0051" in satisfies or "FEAT_0052" in satisfies or "FEAT_0053" in satisfies or "FEAT_0054" in satisfies or "FEAT_0055" in satisfies or "FEAT_0056" in satisfies or "FEAT_0057" in satisfies

.. toctree::
   :maxdepth: 2

   esi-parser
   ir-backend
   ethercrab-backend
   runtime-trait
   build-helper
   cli
   eeprom-diff
   anti-goals
