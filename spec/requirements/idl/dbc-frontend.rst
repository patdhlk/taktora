DBC frontend
============

The DBC (CAN database) frontend (:need:`BB_0119`): parses ``.dbc`` text
into a typed model and lowers it onto an ``idl-core`` ``Module`` plus a
physical-layout sidecar. DBC is the natural first frontend because it is
bounded by construction — every message has a fixed DLC and every signal
a fixed bit width.

.. feat:: DBC frontend
   :id: FEAT_0113
   :status: implemented
   :satisfies: FEAT_0110

   A new crate ``taktora-idl-dbc`` that parses ``.dbc`` databases and
   lowers them onto the message-plane IR. It proves the
   description → IR → codegen pipeline against a frontend whose
   boundedness invariant is never even at risk.

.. req:: DBC parse to a typed model
   :id: REQ_0952
   :status: implemented
   :satisfies: FEAT_0113
   :links: BB_0119, TEST_0929

   ``taktora-idl-dbc`` shall expose ``parse(text) -> Result<DbcDatabase,
   ParseError>`` producing a faithful in-memory model of the database:
   messages (with CAN id, extended-id flag, DLC, and sender), signals
   (start bit, bit length, byte order, signedness, factor, offset, min,
   max, unit), value tables, and signal multiplexer roles
   (multiplexor / multiplexed / none).

.. req:: DBC lower to IR plus layout sidecar
   :id: REQ_0953
   :status: implemented
   :satisfies: FEAT_0113
   :links: BB_0119, TEST_0929, TEST_0935

   ``taktora-idl-dbc`` shall expose ``lower(&db, module_name) ->
   Result<LoweredDbc, LowerError>`` producing two outputs: an
   ``idl_core::Module`` (each message a struct, each signal a field,
   each value table an enum) and a ``DbcLayout`` sidecar carrying the
   physical placement the IR deliberately excludes (start bit, bit
   length, byte order, factor, offset). The lowered ``Module`` shall
   always pass :need:`REQ_0947`. The sidecar shall be keyed by the same
   source names the ``Module`` carries so a backend can rejoin the
   logical and physical halves. Multiplexed signals shall lower into
   plain fields of one struct this round.
