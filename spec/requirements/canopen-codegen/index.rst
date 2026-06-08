CANopen device-driver codegen
=============================

This chapter captures the requirements for the **CANopen device-driver
codegen toolchain**: a layered set of crates that translates CANopen
Electronic Data Sheet (EDS, CiA 306) files into strongly-typed Rust
driver modules at build time, with zero runtime INI parsing and no
dependency on the ``taktora-connector-can`` runtime.

The decomposition is the peer of :doc:`../../requirements/device-codegen/index` for CANopen,
executing the lift foreseen by :need:`ADR_0073` (now closed by
:need:`ADR_0078`):

* **Top-level umbrella feature** — :need:`FEAT_0060` — peer to
  :need:`FEAT_0050` (EtherCAT codegen). The umbrella is build-time
  only and orthogonal to :need:`FEAT_0046` "CAN reference connector";
  the runtime adapter that wires generated devices into the connector
  is a follow-on spec.
* **Shared OD core** — :need:`FEAT_0061` lifts the OD IR (Identity,
  DictEntry, DataType, PdoEntry, PdoMap, AccessRights) out of
  ``ethercat-esi`` into a new ``fieldbus-od-core`` crate so both
  parsers share it.
* **Capability-cluster sub-features** — one per crate-layer concern,
  each ``:satisfies:`` :need:`FEAT_0060`.
* **Requirements** — concrete shall-clauses that ``:satisfies:`` a
  capability-cluster feature.

This round covers EDS only. DCF (Device Configuration File) support
and a live-bus verifier are explicitly out of scope; the architecture
preserves the option to add either later (see :doc:`anti-goals`).

The umbrella decomposes into eight capability clusters, each on its own
page (see the toctree): the shared OD core (:need:`FEAT_0061`), the EDS
parser (:need:`FEAT_0062`), the codegen IR and backend trait
(:need:`FEAT_0063`), the taktora-connector-can backend
(:need:`FEAT_0064`), the runtime trait surface (:need:`FEAT_0065`), the
build helper (:need:`FEAT_0066`), CLI inspection (:need:`FEAT_0067`),
and EDS ↔ SDO-dump verification (:need:`FEAT_0068`). The deliberately
rejected anti-goals and the umbrella-level traceability tables live on
:doc:`anti-goals`.

Top-level umbrella
------------------

.. feat:: CANopen device-driver codegen toolchain
   :id: FEAT_0060
   :status: open

   A layered set of Rust crates that consumes CANopen EDS files (CiA
   306) and emits strongly-typed driver modules at build time. The
   toolchain is organised as five layers that depend only leftwards:

   1. **Shared OD core** — ``fieldbus-od-core``: OD IR lifted from
      ``ethercat-esi``. ``no_std`` + ``alloc``. Knows no XML, no INI,
      no transport.
   2. **Parse layer** — ``canopen-eds``: CiA 306 INI → typed IR.
      Depends on ``fieldbus-od-core``. No codegen, no transport dep.
   3. **Codegen layer** — ``canopen-eds-codegen`` (IR →
      ``TokenStream`` via ``CodegenBackend`` trait) plus
      ``canopen-eds-codegen-taktora`` (the one concrete backend this
      round, targeting the ``CanOpenDevice`` trait surface).
   4. **Runtime trait crate** — ``canopen-eds-rt``: the
      ``CanOpenDevice`` / ``CanOpenConfigurable`` traits the
      generated drivers implement. Frame-per-PDO dispatch — no
      cyclic process-image model (per :need:`REQ_0592`).
   5. **Tooling layer** — ``canopen-eds-build`` (build.rs glue),
      ``canopen-eds-cli`` (``cargo eds expand`` / ``cargo eds list``
      one-shot tools), and ``canopen-eds-verify`` (offline diff of
      EDS XML against a captured SDO-upload JSON dump).

   The ``taktora-connector-can`` crate (see :need:`FEAT_0046`) is not
   part of this toolchain. A thin adapter that maps any
   ``CanOpenDevice`` into the connector's frame plumbing is a
   follow-on spec; this umbrella does not require changes to
   :need:`FEAT_0046`'s runtime contracts (see :need:`REQ_0795`).

Requirements at a glance
------------------------

.. needtable::
   :columns: id, title, status, satisfies
   :show_filters:
   :filter: "FEAT_0060" in satisfies or "FEAT_0061" in satisfies or "FEAT_0062" in satisfies or "FEAT_0063" in satisfies or "FEAT_0064" in satisfies or "FEAT_0065" in satisfies or "FEAT_0066" in satisfies or "FEAT_0067" in satisfies or "FEAT_0068" in satisfies

.. toctree::
   :maxdepth: 2

   od-core
   eds-parser
   ir-backend
   can-backend
   runtime-trait
   build-helper
   cli
   sdo-verify
   anti-goals
