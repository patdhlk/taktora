Network-config parser and IR
============================

Parser crate requirements for :need:`FEAT_0081` — reading
``network.yaml`` into a typed IR that is independent of code emission.

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
   :status: implemented
   :satisfies: FEAT_0081
   :links: BB_0124, TEST_0830

   The crate shall expose a parse entry point that deserialises a
   ``network.yaml`` document into a typed ``NetworkConfig`` IR via
   ``serde``. Parsing the YAML text itself shall perform no code
   emission; resolution of referenced ESI files is the parser's only
   filesystem access.

.. req:: IR carries bus config, device instances, and channel bindings
   :id: REQ_0821
   :status: implemented
   :satisfies: FEAT_0081
   :links: BB_0124, TEST_0830

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
   :status: implemented
   :satisfies: FEAT_0081
   :links: BB_0124, TEST_0831

   One ``network.yaml`` document shall describe exactly one EtherCAT
   bus — one connector, one NIC, one process image. Multi-bus documents
   shall be rejected by the parser. Multiple buses are expressed as
   multiple files producing multiple generated modules (see
   :need:`ADR_0096`).

.. req:: Devices referenced by stable label, not address
   :id: REQ_0823
   :status: implemented
   :satisfies: FEAT_0081
   :links: BB_0124, TEST_0832

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
