Runtime trait surface
=====================

The minimal trait pair (:need:`BB_0084`) the generated devices implement
and any downstream CAN adapter consumes. Lives in a tiny
``canopen-eds-rt`` crate so the runtime contract is not coupled to
either the codegen or the connector.

.. feat:: Runtime trait surface
   :id: FEAT_0065
   :status: open
   :satisfies: FEAT_0060

   The minimal trait pair the generated devices implement and any
   downstream CAN adapter consumes. Lives in a tiny
   ``canopen-eds-rt`` crate so the runtime contract is not coupled
   to either the codegen or the connector.

.. req:: CanOpenDevice trait shape
   :id: REQ_0750
   :status: open
   :satisfies: FEAT_0065

   The crate shall define a ``CanOpenDevice`` trait with
   ``const IDENTITY: Identity``, ``fn node_id(&self) -> u8``,
   ``fn set_node_id(&mut self, id: u8)``,
   ``fn nmt_state(&self) -> NmtState``,
   ``fn set_nmt_state(&mut self, s: NmtState)``,
   ``fn on_rpdo(&mut self, idx: u8, frame: PdoFrame<'_>) -> Result<(), CanOpenError>``,
   and
   ``fn drain_tpdos(&mut self, out: &mut dyn TpdoSink) -> Result<(), CanOpenError>``.
   The trait shall add no async methods on the RPDO/TPDO path —
   the frame-handling hot path stays synchronous.

.. req:: CanOpenConfigurable trait shape for bring-up
   :id: REQ_0751
   :status: open
   :satisfies: FEAT_0065

   The crate shall define
   ``CanOpenConfigurable: CanOpenDevice`` carrying
   ``async fn configure<S: SdoClient>(&mut self, sdo: &mut S) ->
   Result<(), CanOpenError>``. Bring-up SDO writes (PDO comm,
   PDO mapping, optional NMT start) live inside the generated
   body of this method.

.. req:: Traits live in canopen-eds-rt, not taktora-connector-can
   :id: REQ_0752
   :status: open
   :satisfies: FEAT_0065

   ``CanOpenDevice`` and ``CanOpenConfigurable`` shall live in a
   dedicated ``canopen-eds-rt`` crate. They shall not live in
   ``taktora-connector-can``, ``fieldbus-od-core``, or any other
   taktora-internal crate. Any CAN consumer shall be able to adopt
   the generated drivers without depending on taktora.

.. req:: Frame payloads use heapless::Vec<u8, 8>
   :id: REQ_0753
   :status: open
   :satisfies: FEAT_0065

   ``PdoFrame`` shall borrow an inbound payload as ``&[u8]``;
   ``PdoOut`` shall carry the outbound payload as
   ``heapless::Vec<u8, 8>`` (classical CAN cap). ``PdoOut::can_id:
   u32`` shall carry the resolved COB-ID computed by generated code
   from the current ``node_id()`` and the EDS-declared base
   COB-ID; the consumer forwards it as the CAN frame ID without
   further resolution. CAN-FD payloads are out of scope this round
   (see :need:`REQ_0791`).

.. req:: Frame-per-PDO dispatch shape
   :id: REQ_0754
   :status: open
   :satisfies: FEAT_0065

   ``on_rpdo(idx, frame)`` shall accept an RPDO enumeration index
   in ``0..=3`` (CANopen's 4-RPDO cap from CiA 301) and route to
   the right typed decoder in generated code. Caller-side
   resolution from CAN ID to RPDO enumeration uses the configured
   ``0x1400..0x14FF`` communication parameters and is the
   consumer's responsibility. ``drain_tpdos(out)`` shall produce
   zero or more frames per call into a caller-provided
   ``TpdoSink``.

.. req:: CanOpenError variant surface
   :id: REQ_0755
   :status: open
   :satisfies: FEAT_0065

   ``CanOpenError`` shall enumerate CiA 301 SDO abort codes
   (``AbortCode(u32)``), payload-length mismatch
   (``PdoLenMismatch { expected: u8, got: u8 }``), unknown-index,
   ``NmtStateViolation``, and a
   ``TransportFailed(&'static str)`` variant for caller-supplied
   I/O failures.

.. req:: RPDO rejected outside Operational state
   :id: REQ_0756
   :status: open
   :satisfies: FEAT_0065

   Generated code shall reject ``on_rpdo`` calls when
   ``nmt_state() != NmtState::Operational``, returning
   ``CanOpenError::NmtStateViolation``. NMT state is
   caller-managed; the trait carries getter / setter but no
   transition methods.
