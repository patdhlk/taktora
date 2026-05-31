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
:doc:`device-codegen`: a top-level umbrella feature, capability-cluster
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

Network-config parser and IR
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

.. feat:: Network-config parser and IR
   :id: FEAT_0081
   :status: open
   :satisfies: FEAT_0080

   A parser crate. Reads ``network.yaml``, emits a typed in-memory IR
   (the "Device / SubDevice" data model). Knows nothing about code
   emission. Suitable for any downstream tool — codegen, a topology
   visualiser, a validator.

.. req:: YAML parse to typed network IR
   :id: REQ_0820
   :status: open
   :satisfies: FEAT_0081

   The crate shall expose a parse entry point that deserialises a
   ``network.yaml`` document into a typed ``NetworkConfig`` IR via
   ``serde``. Parsing the YAML text itself shall perform no code
   emission; resolution of referenced ESI files is the parser's only
   filesystem access.

.. req:: IR carries bus config, device instances, and channel bindings
   :id: REQ_0821
   :status: open
   :satisfies: FEAT_0081

   The IR shall represent: a ``BusConfig`` (cycle time, distributed-clocks
   flag, ``max_subdevices`` / ``max_pdi_bytes`` compile-time bounds,
   optional default NIC); a ``Vec<DeviceInstance>`` in bus order, each
   carrying a ``label``, a ``DeviceSource`` (``Esi { path, pinned_hash,
   revision }`` or ``Inline { rx, tx }``), an optional ``Identity``, an
   optional ``station_alias``, and an optional ``address`` override; and a
   ``Vec<ChannelBinding>`` carrying channel name, device label,
   direction, bit offset, bit length, element type, and an
   ``allow_overlap`` flag.

.. req:: One file describes exactly one bus
   :id: REQ_0822
   :status: open
   :satisfies: FEAT_0081

   One ``network.yaml`` document shall describe exactly one EtherCAT
   bus — one connector, one NIC, one process image. Multi-bus documents
   shall be rejected by the parser. Multiple buses are expressed as
   multiple files producing multiple generated modules (see
   :need:`ADR_0096`).

.. req:: Devices referenced by stable label, not address
   :id: REQ_0823
   :status: open
   :satisfies: FEAT_0081

   Each device instance shall carry a stable string ``label``. Channel
   bindings shall reference their device by ``label``, never by raw
   configured address or list index, so that the configured address
   (assigned per :need:`REQ_0825`) remains a derived value and reordering
   devices does not require editing channel bindings.

.. req:: Parser depends on ethercat-esi, never on the connector runtime
   :id: REQ_0824
   :status: open
   :satisfies: FEAT_0081

   The ``ethercat-netcfg`` crate shall depend on ``ethercat-esi`` and
   ``fieldbus-od-core`` (to resolve ESI references and validate inline
   offsets against a device's real PDO layout) and shall not declare
   ``taktora-connector-ethercat`` as a dependency.

Codegen
~~~~~~~

.. feat:: Network-config codegen
   :id: FEAT_0082
   :status: open
   :satisfies: FEAT_0080

   A codegen crate translating the ``NetworkConfig`` IR into a
   ``TokenStream`` of ``&'static`` bus tables and named routing
   constants, formatted with ``prettyplease`` for byte-stable,
   reviewable output.

.. req:: Emit static SubDeviceMap PDO tables
   :id: REQ_0825
   :status: open
   :satisfies: FEAT_0082

   Codegen shall emit a ``pub static PDO_MAP: &[SubDeviceMap]`` whose
   entries carry each device's computed configured address, mapped
   RxPDO / TxPDO ``PdoEntry`` slices, and a derived ``expected_wkc``
   (per :need:`REQ_0828`). The emitted types shall be the existing
   ``taktora_connector_ethercat`` types, named textually.

.. req:: Emit named routing and channel-name constants
   :id: REQ_0826
   :status: open
   :satisfies: FEAT_0082

   Codegen shall emit, per channel binding, a named ``EthercatRouting``
   constant carrying the resolved subdevice address, direction, bit
   offset, and bit length, plus the channel-name string constant. The
   element type shall be a primitive (inline case) or an ESI-derived
   type.

.. req:: Configured addresses assigned by bus position
   :id: REQ_0827
   :status: open
   :satisfies: FEAT_0082

   Codegen shall assign each device's configured station address as
   ``0x1000 + n`` where ``n`` is its zero-based position in the
   bus-ordered device list, mirroring ``init_single_group``. An explicit
   per-device ``address`` override shall take precedence when present and
   is reserved for bus segments the integrator does not control.

.. req:: Working-counter expectation derived, never overridden
   :id: REQ_0828
   :status: open
   :satisfies: FEAT_0082

   The ``expected_wkc`` for each SubDevice shall be derived solely from
   its mapped PDO directions (the canonical 0/1/2/3 rule). The toolchain
   shall provide no mechanism to override the derived value; there is
   exactly one source of truth.

.. req:: Generated output is byte-deterministic
   :id: REQ_0829
   :status: open
   :satisfies: FEAT_0082

   The same ``network.yaml`` plus the same pinned ESI inputs shall
   produce a byte-identical generated module across machines and
   toolchain versions. Timestamps, hash-map iteration order, and source
   ordering shall not leak into the output.

Build glue
~~~~~~~~~~

.. feat:: Build-script glue
   :id: FEAT_0083
   :status: open
   :satisfies: FEAT_0080

   A ``build.rs`` helper that wires the parser and codegen into a
   consuming crate's build, emitting the generated module into
   ``OUT_DIR``.

.. req:: Generate into OUT_DIR for include
   :id: REQ_0830
   :status: open
   :satisfies: FEAT_0083

   The build helper shall read the configured ``network.yaml``, run
   parse + codegen, and write one Rust module into ``OUT_DIR`` for the
   consumer to pull in via ``include!(concat!(env!("OUT_DIR"),
   "/network.rs"))``. The generated module shall not be checked into
   version control.

.. req:: Rebuild on config or ESI change
   :id: REQ_0831
   :status: open
   :satisfies: FEAT_0083

   The build helper shall emit ``cargo:rerun-if-changed`` directives for
   the ``network.yaml`` and every vendored ESI file it resolves, so a
   change to topology or a referenced device description triggers
   regeneration.

CLI and vendoring
~~~~~~~~~~~~~~~~~

.. feat:: CLI and ESI vendoring
   :id: FEAT_0084
   :status: open
   :satisfies: FEAT_0080

   A command-line surface for inspecting generated output and for the
   deliberate vendor-and-pin action that brings remote ESI files local.

.. req:: Expand subcommand prints generated module
   :id: REQ_0832
   :status: open
   :satisfies: FEAT_0084

   The CLI shall provide a ``netcfg expand`` subcommand that prints the
   generated module to stdout for inspection and diffing, mirroring the
   ESI toolchain's ``cargo esi expand`` (see :need:`ADR_0077`).

.. req:: Fetch subcommand vendors and pins remote ESI
   :id: REQ_0833
   :status: open
   :satisfies: FEAT_0084

   The CLI shall provide a ``netcfg fetch`` subcommand that resolves a
   web-URL ESI reference once, downloads it into a local vendored
   directory, and records its content hash and device revision into a
   lockfile beside the ``network.yaml``.

.. req:: Build resolves ESI from local files only
   :id: REQ_0834
   :status: open
   :satisfies: FEAT_0084

   The build path shall resolve ESI references from local files only. A
   web-URL reference with no matching vendored, pinned local file shall
   be a build error, never a live network fetch. Builds shall be
   hermetic, reproducible, and runnable air-gapped.

.. req:: ESI references pinned by content hash and revision
   :id: REQ_0835
   :status: open
   :satisfies: FEAT_0084

   Every ESI reference shall be pinned by content hash and device
   revision. A mismatch between a resolved ESI file and its pinned hash
   or revision shall be a build error (per :need:`REQ_0837`), so a
   silently re-published vendor file surfaces as a visible diff rather
   than a behaviour change.

Validation and bring-up assertions
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

.. feat:: Validation and bring-up assertions
   :id: FEAT_0085
   :status: open
   :satisfies: FEAT_0080

   Build-time validation of everything derivable from the YAML + ESI,
   plus generation of the bring-up assertions that check facts only the
   physical bus can confirm.

.. req:: Hard build errors for derivable faults
   :id: REQ_0836
   :status: open
   :satisfies: FEAT_0085

   Codegen shall fail the build on: two routings overlapping the same
   bit range in the same SubDevice and direction without
   ``allow_overlap``; a slice extending past the device's declared / ESI
   process-image size; ``bit_length == 0``; a channel referencing a
   non-existent device label; a channel-name or (override-induced)
   configured-address collision; and an ESI contradiction (a device with
   both an ESI reference and disagreeing inline offsets, or an ESI
   hash / revision not matching the lockfile).

.. req:: Warn on unmapped process-image gaps
   :id: REQ_0837
   :status: open
   :satisfies: FEAT_0085

   Codegen shall emit a non-fatal warning for unmapped bit ranges within
   a device's process image. Gaps are legal and often intentional, but
   shall never be silent.

.. req:: Emit bring-up assertions for physical-bus facts
   :id: REQ_0838
   :status: open
   :satisfies: FEAT_0085

   For facts that can only be checked against the physical bus, codegen
   shall emit data driving runtime bring-up assertions: a per-position
   device-identity table (vendor id / product code / revision), the
   declared ``station_alias`` values, and the derived ``expected_wkc``.
   A mismatch at bring-up — wrong device identity or alias at a position,
   or a live working counter diverging from the expectation — shall drive
   the connector's existing ``Degraded`` / ``Down`` health path rather
   than mirroring process data to the wrong terminal.

.. req:: No runtime parsing, no connector-runtime modification
   :id: REQ_0839
   :status: open
   :satisfies: FEAT_0085

   The toolchain shall introduce no runtime YAML parsing and no
   per-instance heap for the bus configuration: all configuration
   resolves at build time into ``&'static`` tables. No change to the
   ``taktora-connector-ethercat`` runtime contracts shall be required to
   consume the generated module.

Requirements at a glance
------------------------

.. needtable::
   :types: req, feat
   :columns: id, title, status, satisfies
   :filter: "netcfg" in title or id in ["FEAT_0080", "FEAT_0081", "FEAT_0082", "FEAT_0083", "FEAT_0084", "FEAT_0085"]
   :show_filters:
