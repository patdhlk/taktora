Message-plane interface-description codegen
===========================================

This chapter captures the requirements for the **message-plane
interface-description codegen toolchain**: a layered set of crates that
translates interface descriptions (today, CAN ``.dbc`` databases) into
strongly-typed Rust message (de)serializers at build time, with a
``no_std``, allocation-free, ``serde``-free runtime (de)serialization
path.

The decomposition is the **message-plane twin** of
:doc:`../../requirements/device-codegen/index` (the device-plane ESI
toolchain) and :doc:`../../requirements/canopen-codegen/index`. Where the
device-plane toolchains describe a *device* (identity + object dictionary
+ cyclic process image), this toolchain describes a *message* (the
structs, enums, and request/reply services that cross a channel). The
bounded message-type IR (:need:`BB_0117`) is the peer of
``fieldbus-od-core``:

* **Top-level umbrella feature** — :need:`FEAT_0110` — peer to
  :need:`FEAT_0050` (EtherCAT device codegen) and :need:`FEAT_0060`
  (CANopen device codegen). Build-time only; the only crate that links
  into a consumer at runtime is the wire runtime (:need:`FEAT_0112`).
* **Capability-cluster sub-features** — one per crate-layer concern,
  each ``:satisfies:`` :need:`FEAT_0110`.
* **Requirements** — concrete shall-clauses that ``:satisfies:`` a
  capability-cluster feature.

This round delivers the DBC frontend and the CAN/wire backend
end-to-end. OMG IDL and ROS 2 ``.msg``/``.srv`` frontends, and a
J1939-PGN consumer of the generated types, are foreseen but out of scope
this round; the architecture keeps them net-additive (see
:doc:`anti-goals`).

The umbrella decomposes into five capability clusters, each on its own
page (see the toctree): the bounded message-type IR (:need:`FEAT_0111`),
the ``serde``-free wire runtime (:need:`FEAT_0112`), the DBC frontend
(:need:`FEAT_0113`), the plane-generic codegen (:need:`FEAT_0114`), and
the CAN/DBC backend (:need:`FEAT_0115`). The deliberately deferred
anti-goals live on :doc:`anti-goals`.

Top-level umbrella
------------------

.. feat:: Message-plane interface-description codegen
   :id: FEAT_0110
   :status: implemented
   :links: BB_0117, BB_0118, BB_0119, BB_0114, BB_0115

   A layered set of Rust crates that consumes interface descriptions and
   emits strongly-typed message (de)serializers at build time. The
   toolchain is organised as layers that depend only leftwards:

   1. **IR core** — ``taktora-idl-core``: the bounded message-type IR
      (structs, enums, bounded sequences, services). ``std`` baseline.
      Knows no wire format, no DBC, no target language. The message-plane
      twin of ``fieldbus-od-core``.
   2. **Wire runtime** — ``taktora-idl-wire``: the ``WireType`` trait the
      generated code implements, plus CAN signal bit-packing primitives.
      ``no_std``, allocation-free, dependency-free, no ``serde``. This is
      the only layer that links into a runtime consumer.
   3. **Frontend** — ``taktora-idl-dbc``: parses ``.dbc`` text into a
      typed model and lowers it onto an ``idl-core`` ``Module`` plus a
      physical-layout sidecar. Depends on ``idl-core``.
   4. **Codegen layer** — ``taktora-idl-codegen`` (naming policy, the
      ``MessageBackend`` trait, and ``resolve`` + ``generate``;
      plane-generic, knows no wire format) plus
      ``taktora-idl-codegen-can`` (the one concrete backend this round,
      emitting ``WireType`` impls per the DBC layout).
   5. **Verification harness** — ``taktora-idl-codegen-can-tests``
      (``publish = false``): generates ``WireType`` code from a fixture
      DBC at build time and round-trips it.

   The delivered slice is the IR plus the DBC frontend and CAN backend.
   Additional frontends (OMG IDL, ROS 2) and a J1939 consumer are
   net-additive follow-ons (see :doc:`anti-goals`).

Requirements at a glance
------------------------

.. needtable::
   :columns: id, title, status, satisfies
   :show_filters:
   :filter: "FEAT_0110" in satisfies or "FEAT_0111" in satisfies or "FEAT_0112" in satisfies or "FEAT_0113" in satisfies or "FEAT_0114" in satisfies or "FEAT_0115" in satisfies

.. toctree::
   :maxdepth: 2

   ir-core
   wire-runtime
   dbc-frontend
   codegen
   can-backend
   anti-goals
