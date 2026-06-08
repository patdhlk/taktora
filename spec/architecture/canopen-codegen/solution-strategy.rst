Solution strategy
=================

arc42 §5 — the toolchain's shape is the consequence of nine
architectural decisions captured below. The structure of the decisions mirrors
:doc:`../device-codegen/solution-strategy` so the rationale chain from ESI to EDS is
explicit and visible.

.. arch-decision:: Lift OD IR to fieldbus-od-core now
   :id: ADR_0078
   :status: open
   :refines: FEAT_0061
   :links: ADR_0073

   **Context.** :need:`ADR_0073` foresaw the OD-IR lift but left it
   open. Two paths were possible: (a) lift now as part of the
   CANopen codegen round, (b) ship CANopen with a duplicated OD IR
   and lift later.

   **Decision.** Lift now. ``fieldbus-od-core`` is created as a new
   crate; ``ethercat-esi`` shrinks to "ESI-specific elements +
   re-exports from OD core"; ``canopen-eds`` parses against the same
   IR.

   **Consequences.** ✅ Parser cost amortised over both fieldbuses.
   ✅ Closes :need:`ADR_0073`. ✅ No future breaking-change cycle to
   lift later. ❌ One mechanical refactor on existing
   :need:`FEAT_0050` crates. The lift is low-risk because parser and
   IR were already decoupled (per :need:`ADR_0070`).

.. arch-decision:: fieldbus-od-core stays data-only
   :id: ADR_0079
   :status: open
   :refines: FEAT_0061

   **Context.** Possible additions to a shared OD crate include
   built-in serde derives, ``Hash`` derives, INI / XML helpers,
   proc-macro support.

   **Decision.** ``fieldbus-od-core`` is data-only, ``no_std +
   alloc``, no proc-macro. Type derives (``Serialize``,
   ``Deserialize``, ``Hash``) sit behind opt-in cargo features so
   embedded consumers don't pay for them.

   **Consequences.** ✅ Smallest possible blast radius for both
   parsers. ✅ Stable surface — adding a derive is additive. ❌ Two
   consumers (parsers) carry their own serde wiring; acceptable
   since the serde frontends are different anyway (XML vs INI).

.. arch-decision:: Re-export from ethercat-esi, do not break it
   :id: ADR_0080
   :status: open
   :refines: FEAT_0061

   **Context.** :need:`FEAT_0050` is already shipped. Two options
   for compatibility: (a) break the API and bump major version,
   (b) re-export the lifted types from ``ethercat-esi`` as a thin
   façade.

   **Decision.** Re-export. ``ethercat-esi::Identity``,
   ``ethercat-esi::DictEntry``, etc. continue to be valid paths;
   they resolve to ``fieldbus_od_core::*`` under the hood. The
   façade is permanent, not deprecated.

   **Consequences.** ✅ Existing :need:`FEAT_0050` consumers
   compile source-unchanged. ✅ No major version bump needed. ❌
   Two paths exist for the same type. Acceptable — the canonical
   path is documented (``fieldbus_od_core``) and the façade is the
   compatibility seam.

.. arch-decision:: INI backend choice — serde-derive façade
   :id: ADR_0081
   :status: open
   :refines: FEAT_0062

   **Context.** Two reasonable INI crates exist in the Rust
   ecosystem with passive (no-I/O) APIs: ``serde_ini`` and
   ``rust-ini``. Both can drive a serde-derive frontend.

   **Decision.** Treat them as interchangeable behind a serde-derive
   façade. ``serde_ini`` is the primary candidate. If the chosen
   crate cannot satisfy line/column error reporting
   (:need:`REQ_0723`), the alternative is acceptable so long as the
   serde-derive surface is preserved.

   **Consequences.** ✅ The IR is decoupled from the INI tokeniser
   choice. ✅ Backend can be flipped without IR churn. ❌ The choice
   is deferred to the planning phase; the spec does not fix it.

.. arch-decision:: PDO entry dedup is structural, name-blind
   :id: ADR_0082
   :status: open
   :refines: FEAT_0063

   **Context.** When two devices' PDOs carry the same bit-len +
   data-type tuple list but different field names (e.g.
   ``ControlWord`` vs. ``StatusWord1``), the dedup question is
   whether names matter.

   **Decision.** Structural dedup only — names are not part of the
   dedup key (per :need:`REQ_0733`). The EtherCAT side made the
   same call implicitly; this ADR captures it explicitly for both
   fieldbuses going forward.

   **Consequences.** ✅ Higher dedup hit rate across devices that
   share a CiA profile (e.g. CiA 402 servo drives). ✅ Smaller
   generated artefacts. ❌ Two devices' identical-shaped PDO
   structs may have field names from one device only. Acceptable —
   downstream code accesses by position via typed getter, not by
   the EDS-side string.

.. arch-decision:: Dummy entries skip into bit offsets, not padding fields
   :id: ADR_0083
   :status: open
   :refines: FEAT_0064

   **Context.** CANopen permits ``Dummy*`` data-type entries
   (e.g. ``DummyUInt32``) in PDO mappings to pad bit positions
   without binding a real OD object. Three modelling options:
   named padding fields (``pub _pad_0: u32``), unnamed tuple
   padding, or skip entirely.

   **Decision.** Skip. Typed PDO structs carry only real-payload
   fields; bit offsets are threaded through generated ``decode`` /
   ``encode`` bodies (per :need:`REQ_0744`). Padding bits are
   zero-initialised on encode and ignored on decode.

   **Consequences.** ✅ Cleaner API surface. ✅ No temptation for
   callers to write padding fields. ❌ ``decode`` / ``encode`` body
   complexity carries the bit-offset arithmetic; acceptable cost.

.. arch-decision:: heapless::Vec<u8, 8> for PdoOut payload
   :id: ADR_0084
   :status: open
   :refines: FEAT_0065

   **Context.** Outbound TPDO frames need a payload buffer. Three
   options: ``Vec<u8>`` (allocates per frame), fixed-array
   ``[u8; 8]`` (fixed length, can't represent shorter PDOs),
   ``heapless::Vec<u8, 8>`` (no-alloc, capacity bound, length-tracked).

   **Decision.** ``heapless::Vec<u8, 8>``. Matches classical CAN's
   8-byte payload cap; gives length-aware encode that supports
   PDOs shorter than 8 bytes; keeps the ``no_std`` story clean.

   **Consequences.** ✅ No per-frame allocation. ✅ Sound across
   embedded targets without a global allocator. ❌ CAN-FD's
   64-byte payload would need ``heapless::Vec<u8, N>`` with a
   const generic; deferred per :need:`REQ_0791`. The migration is
   mechanical: type-parameterise ``PdoOut`` over its capacity.

.. arch-decision:: Async only on configure, sync on frame path
   :id: ADR_0085
   :status: open
   :refines: FEAT_0065

   **Context.** The trait surface must distinguish hot-path frame
   plumbing (``on_rpdo``, ``drain_tpdos``) from one-shot bring-up
   (``configure``). Three options: all-sync (forces sync SDO),
   all-async (drags an executor into the cycle loop), mixed.

   **Decision.** Mixed. ``on_rpdo`` and ``drain_tpdos`` stay
   synchronous; ``configure`` is async. The caller's tokio (or
   embassy, or whatever) runtime drives bring-up; the frame hot
   path runs in whatever scheduler the consumer chooses.

   **Consequences.** ✅ No runtime dependency leaks into the hot
   path. ✅ Same posture as ``ethercat-esi-rt``'s sync decode /
   encode (per :need:`REQ_0530`). ❌ Caller must arrange an
   ``SdoClient`` impl that completes ``await`` against its CAN
   transport. Acceptable — same shape any CAN runtime adapter
   needs anyway.

.. arch-decision:: JSON SDO-dump format with versioned schema
   :id: ADR_0086
   :status: open
   :refines: FEAT_0068

   **Context.** The verifier needs a dump format to compare EDS
   against. Four options: CSV (lossy on hex / type info), custom
   binary (opaque to git review), YAML (heavier dep), JSON
   (inspectable, diff-able, schema-tag-able).

   **Decision.** JSON with explicit ``schema`` version string
   (``taktora.canopen.sdo-dump.v1``). Per :need:`REQ_0784`. Unknown
   schema strings reject before any field comparison runs.

   **Consequences.** ✅ Inspectable in git diffs. ✅ Easy to
   produce from any tool (Python ``canopen``, shell scripts over
   ``candump``, future ``taktora-connector-can`` adapter). ✅
   Versioned — schema evolution is non-breaking. ❌ One more
   serde-json dep on the verifier; trivial cost.
