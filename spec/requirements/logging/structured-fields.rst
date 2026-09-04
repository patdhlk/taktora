Structured key-value fields
===========================

How ``log::kv`` structured pairs surface as DLT verbose arguments so
DLT Viewer / dlt-tui can index on them, rather than being collapsed
into the formatted message (a refinement of the DLT backend,
:need:`FEAT_0072`).

.. feat:: Structured key-value fields mapped to DLT verbose args
   :id: FEAT_0074
   :status: implemented
   :satisfies: FEAT_0072

   The ``log`` crate (v0.4.21+) supports structured key-value pairs
   alongside the formatted message. ``taktora-log-dlt`` shall surface
   those pairs as DLT verbose arguments rather than collapsing them
   into the formatted message, so DLT Viewer / dlt-tui can index on
   them.

.. req:: log::kv pairs encoded as DLT verbose arguments
   :id: REQ_0809
   :status: implemented
   :satisfies: FEAT_0074
   :links: BB_0091, TEST_0809

   For each ``log::Record``, the DLT backend shall iterate
   ``record.key_values()`` and emit one DLT verbose argument per
   structured pair. Native mappings shall apply where the value's
   type matches a DLT primitive: ``u32`` / ``i32`` / ``u64`` / ``i64``
   / ``f32`` / ``f64`` / ``bool`` / ``&str``. Values whose type does
   not have a native DLT mapping shall be rendered via ``Display`` and
   encoded as a verbose string argument. The formatted message body
   shall always be emitted as the leading argument regardless of
   structured-field presence.
