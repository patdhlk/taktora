Network-config codegen
======================

Codegen crate requirements for :need:`FEAT_0082` — translating the
``NetworkConfig`` IR into ``&'static`` bus tables and routing constants.

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
