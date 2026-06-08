Shared OD core
==============

The shared object-dictionary IR (:need:`BB_0080`): OD types lifted out
of ``ethercat-esi`` so both the EtherCAT and CANopen parsers share the
same IR.

.. feat:: Shared OD core
   :id: FEAT_0061
   :status: open
   :satisfies: FEAT_0060

   A new crate ``fieldbus-od-core`` carrying the OD types both ESI
   and EDS parsers need (CiA 301 semantics). Lifted out of
   ``ethercat-esi`` so both fieldbuses parse against the same IR.
   Executes the lift foreseen by :need:`ADR_0073`.

.. req:: No transport-specific types in fieldbus-od-core
   :id: REQ_0700
   :status: open
   :satisfies: FEAT_0061

   ``fieldbus-od-core`` shall declare no transport-specific types.
   The crate shall not name ``ethercrab``, ``socketcan``,
   ``taktora_connector_*``, or any I/O-bearing crate as a dependency.

.. req:: no_std + alloc, no mandatory serde
   :id: REQ_0701
   :status: open
   :satisfies: FEAT_0061

   The crate shall be ``#![no_std]`` with an ``alloc`` dependency.
   No ``serde``, no ``quick-xml``, no ``serde-ini`` in the default
   feature set. Type derives (``Serialize``, ``Deserialize``,
   ``Hash``) shall sit behind opt-in cargo features so embedded
   consumers do not pay for them.

.. req:: OD type surface
   :id: REQ_0702
   :status: open
   :satisfies: FEAT_0061

   The crate shall carry ``Identity`` (``vendor_id``,
   ``product_code``, ``revision`` — all ``u32``), ``DataType``
   (enumerating the CiA 301 data-type table), ``AccessRights``
   (``Const`` / ``ReadOnly`` / ``WriteOnly`` / ``ReadWrite``),
   ``DictEntry`` (index, sub_index, name, data_type, access,
   default/min/max bytes), ``PdoEntry`` (index, sub_index, bit_len,
   optional name), and ``PdoMap`` (assigned-to OD index plus entry
   list).

.. req:: ethercat-esi re-exports lifted types
   :id: REQ_0703
   :status: open
   :satisfies: FEAT_0061

   ``ethercat-esi`` shall re-export ``Identity``, ``DataType``,
   ``AccessRights``, ``DictEntry``, ``PdoEntry``, and ``PdoMap``
   from ``fieldbus-od-core`` so existing :need:`FEAT_0050`-era
   consumers compile source-unchanged. The re-export façade shall
   stay in place permanently — it is not deprecated.

.. req:: canopen-eds uses fieldbus-od-core types
   :id: REQ_0704
   :status: open
   :satisfies: FEAT_0061

   ``canopen-eds`` shall use ``fieldbus-od-core`` types for every
   OD-shaped field in its IR. The crate shall not redefine
   ``Identity``, ``DictEntry``, ``PdoEntry``, or ``PdoMap``
   locally.
