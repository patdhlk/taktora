Runtime trait surface
=====================

The minimal trait pair the generated devices implement and the
connector adapter consumes, in the tiny ``ethercat-esi-rt`` crate
(:need:`BB_0063`) — see :need:`ADR_0098`.

.. feat:: Runtime trait surface
   :id: FEAT_0054
   :status: open
   :satisfies: FEAT_0050

   The minimal trait pair the generated devices implement and the
   ``taktora-connector-ethercat`` adapter consumes. Lives in a tiny
   ``ethercat-esi-rt`` crate so the runtime contract is not coupled
   to either the codegen or the connector.

.. req:: EsiDevice trait shape
   :id: REQ_0530
   :status: implemented
   :satisfies: FEAT_0054
   :links: BB_0063, TEST_0430

   The crate shall define an **object-safe** (``dyn``-compatible)
   ``EsiDevice`` trait with ``fn identity(&self) -> Identity``,
   ``fn input_len(&self) -> usize``, ``fn output_len(&self) ->
   usize``, ``fn decode_inputs(&mut self, bits: &BitSlice<u8, Lsb0>)
   -> Result<(), EsiError>``, and ``fn encode_outputs(&self, bits:
   &mut BitSlice<u8, Lsb0>) -> Result<(), EsiError>``. ``input_len``
   / ``output_len`` are in **bytes** (the sync-manager-mapped
   process-image size, rounded up to a byte boundary).

   Identity is exposed as a **method, not an associated**
   ``const`` — an associated const would make the trait
   ``dyn``-incompatible and break the ``Box<dyn EsiDevice>`` registry
   factories of :need:`REQ_0525`. The static identity value is the
   standalone ``const`` emitted by :need:`REQ_0522`; the generated
   ``identity()`` returns it. ``Identity`` is the shared
   ``taktora-fieldbus-od-core`` type (:need:`ADR_0078`). The
   ``EsiError`` here is a *runtime* decode/encode error owned by
   ``ethercat-esi-rt`` (e.g. PDI bit-slice too short), distinct from
   the parser's parse-time ``EsiError`` (:need:`REQ_0506`). The trait
   shall add no async methods — the cyclic hot path stays
   synchronous. See :need:`ADR_0098`.

.. req:: EsiConfigurable trait shape for preop bring-up
   :id: REQ_0531
   :status: open
   :satisfies: FEAT_0054

   The crate shall define ``EsiConfigurable: EsiDevice`` with
   ``type Assignment`` and ``async fn configure<S: SdoWrite>(&mut
   self, sub: &S, a: Self::Assignment) -> Result<(), EsiError>``. The
   bring-up path is generic over the minimal ``SdoWrite`` trait
   (:need:`REQ_0535`) rather than naming ethercrab's concrete
   ``SubDevicePreOperational``, so ``ethercat-esi-rt`` carries **no
   ethercrab dependency** and stays adoptable by non-ethercrab
   consumers and unit-testable against a mock ``SdoWrite``. The
   concrete ``impl SdoWrite for SubDevicePreOperational`` lives in the
   ethercrab backend (:need:`REQ_0520`). Bring-up SDO writes
   (InitCmds, 0x1C12 / 0x1C13 PDO assignment writes) live inside the
   generated body of this method. See :need:`ADR_0098`.

.. req:: SdoWrite abstraction keeps ethercrab out of the trait crate
   :id: REQ_0535
   :status: open
   :satisfies: FEAT_0054

   ``ethercat-esi-rt`` shall define a minimal ``SdoWrite`` trait —
   ``async fn sdo_write(&self, index: u16, sub_index: u8, data:
   &[u8]) -> Result<(), Self::Error>`` with an associated
   ``type Error`` — that abstracts the single ethercrab touchpoint
   used by :need:`REQ_0531`'s ``configure``. The runtime-trait crate
   shall not depend on ``ethercrab``; the concrete
   ``impl SdoWrite for SubDevicePreOperational`` is provided by the
   ethercrab backend (:need:`REQ_0520`), preserving the "only the
   backend touches ethercrab" invariant. ``SdoWrite`` errors surface
   into the runtime ``EsiError`` (e.g. an ``EsiError::Sdo`` variant).

.. req:: Traits live in ethercat-esi-rt, not taktora-connector
   :id: REQ_0532
   :status: open
   :satisfies: FEAT_0054

   ``EsiDevice`` and ``EsiConfigurable`` shall live in a dedicated
   ``ethercat-esi-rt`` crate. They shall not live in
   ``taktora-connector-ethercat``, ``ethercat-hal``, or any other
   taktora-internal crate, so any ethercrab user can adopt the
   generated drivers without depending on taktora.

.. req:: Object dictionary emission is a default-off cargo feature
   :id: REQ_0533
   :status: open
   :satisfies: FEAT_0054

   Emission of the full object-dictionary table per device (as a
   ``static &[(u16, u8, DataType, &str)]`` lookup, per
   :need:`ADR_0075`) shall be gated behind a default-off
   ``object-dictionary`` cargo feature on the generated module's
   parent crate. PDOs and InitCmd writes shall remain unconditional;
   only the OD table is gated. With the feature off, generated code
   shall not pay for OD blow-up (which can reach 10–50× for OD-heavy
   devices per :need:`RISK_0010`).

.. req:: Process image access via bitvec BitSlice
   :id: REQ_0534
   :status: implemented
   :satisfies: FEAT_0054
   :links: BB_0063, TEST_0430

   ``decode_inputs`` and ``encode_outputs`` shall operate on
   ``bitvec::slice::BitSlice<u8, Lsb0>`` references covering the
   device's portion of the EtherCAT cycle PDI. The trait shall not
   embed an opinion on how the surrounding application acquires
   those slices.
