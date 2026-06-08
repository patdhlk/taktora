EDS parser
==========

The pure parse layer (:need:`BB_0081`): CiA 306 INI in, a typed
in-memory IR out, with no knowledge of codegen, transports, or
taktora-internal crates.

.. feat:: EDS parser
   :id: FEAT_0062
   :status: open
   :satisfies: FEAT_0060

   A pure parser crate. Reads CiA 306 INI, emits a typed IR rooted
   in :need:`FEAT_0061` types. Knows nothing about codegen,
   transports, or taktora-internal crates. Suitable for any downstream
   tool — codegen, network configurator, simulator, verifier.

.. req:: Pure parse function with no I/O
   :id: REQ_0720
   :status: open
   :satisfies: FEAT_0062

   The crate shall expose ``parse(text: &str) -> Result<EdsFile,
   EdsError>``. The function shall perform no filesystem or network
   I/O; the caller is responsible for reading the EDS bytes.

.. req:: no_std + alloc, no upstream coupling
   :id: REQ_0721
   :status: open
   :satisfies: FEAT_0062

   The crate shall be ``#![no_std]`` with ``alloc``, and shall not
   depend on ``ethercrab``, ``canopen-eds-codegen``,
   ``taktora-connector-can``, or any transport crate. A downstream
   tool that only needs the IR shall not be forced to compile the
   codegen layer.

.. req:: serde-derive INI backend
   :id: REQ_0722
   :status: open
   :satisfies: FEAT_0062

   The crate shall implement parsing on top of a serde INI
   deserialiser (``serde_ini`` is the primary candidate; the
   alternative ``rust-ini`` is acceptable behind the same façade).
   Hand-written line parsing is rejected — schema maintenance lives
   in the ``serde`` derives.

.. req:: Parse errors carry line and column
   :id: REQ_0723
   :status: open
   :satisfies: FEAT_0062

   ``EdsError`` variants raised during parsing shall carry the
   source line and byte column of the offending construct so
   build-time diagnostics point at the failing EDS file location.

.. req:: Unknown sections captured as RawSection
   :id: REQ_0724
   :status: open
   :satisfies: FEAT_0062

   Unknown EDS sections shall be retained in the IR as
   ``RawSection { name, keys: Vec<(String, String)> }``. The parser
   shall not hard-fail on unknown sections — direct CANopen
   analogue of the EtherCAT ``RawXml`` policy (:need:`ADR_0074`).

.. req:: Liberal parsing — warn and continue on quirks
   :id: REQ_0725
   :status: open
   :satisfies: FEAT_0062

   The parser shall be liberal on common EDS formatting quirks:
   BOM, CRLF/LF mix, trailing whitespace on values, comments after
   values (``; ...``), redundant whitespace around ``=``, and EDS
   exporter-quirk keys (e.g. ``LineFeed``). Quirks shall surface
   as ``EdsFile::warnings: Vec<EdsWarning>`` carrying ``{ line,
   kind }`` so build-time logs can print them without failing the
   parse. A strict mode is not provided in this round.

.. req:: IR carries identity, OD, PDO comm + maps
   :id: REQ_0726
   :status: open
   :satisfies: FEAT_0062

   The IR shall represent, per device: ``Identity`` (lifted from
   the ``[Identity]`` block / OD index ``0x1018``), ``DeviceInfo``
   (vendor / product names, baud-rate flags, NMT-boot-slave flag),
   the OD as ``Vec<DictEntry>``, ``Vec<PdoMap>`` for declared
   RPDOs / TPDOs, and parallel ``Vec<RPdoComm>`` / ``Vec<TPdoComm>``
   for PDO communication parameters (transmission type, cob-id,
   inhibit time, event timer).
