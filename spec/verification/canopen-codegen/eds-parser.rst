EDS parser tests
================

Per-crate, no I/O beyond test fixtures, parallel-safe. Live under
``crates/canopen-eds/tests/``.

.. test:: parse() accepts a representative Maxon EPOS4 EDS
   :id: TEST_0610
   :status: open
   :verifies: REQ_0720, REQ_0722, REQ_0726

   Loads a canonical Maxon ``EPOS4`` EDS fixture from
   ``crates/canopen-eds/tests/fixtures/``, calls ``parse(text)``,
   and asserts the resulting ``EdsFile`` exposes the expected
   identity (``vendor_id = 0x000000FB``), one device, two declared
   TPDOs and two declared RPDOs, the matching communication
   parameter records, and a non-empty object dictionary.

.. test:: Parser compiles under no_std + alloc
   :id: TEST_0611
   :status: open
   :verifies: REQ_0721

   Compile-only test: a small bin target inside
   ``crates/canopen-eds/tests/no_std/`` declares ``#![no_std]``,
   parses a fixture EDS string baked in as ``include_str!``. Must
   compile with ``--no-default-features`` against the crate.

.. test:: Parser is independent of codegen and transport
   :id: TEST_0612
   :status: open
   :verifies: REQ_0721

   CI shell check: ``cargo tree -p canopen-eds`` shall not list
   ``canopen-eds-codegen``, ``canopen-eds-rt``,
   ``taktora-connector-can``, ``socketcan``, or ``ethercrab`` anywhere
   in the resolved graph.

.. test:: Unknown sections survive as RawSection
   :id: TEST_0613
   :status: open
   :verifies: REQ_0724

   Fixture EDS containing a fabricated ``[ManufacturerSpecific_FF]``
   section with two keys. After parsing, the IR carries one
   ``RawSection`` entry whose ``name`` is exactly the section
   header and whose ``keys`` lists both key/value pairs. The parse
   does **not** return an error.

.. test:: Parse errors carry line and column
   :id: TEST_0614
   :status: open
   :verifies: REQ_0723

   A deliberately malformed EDS fixture (missing ``=`` at a known
   line / column) parses to ``Err(EdsError::Syntax { line, column,
   .. })`` with the expected values. Catching this trace in
   build-time output is the user benefit.

.. test:: Liberal-quirk parsing emits warnings without failing
   :id: TEST_0615
   :status: open
   :verifies: REQ_0725

   Fixture EDS carrying every quirk listed in :need:`REQ_0725`:
   leading UTF-8 BOM, CRLF line endings, trailing whitespace on a
   value, ``; comment`` after a value, redundant whitespace around
   ``=``, ``LineFeed=0`` exporter key. The parse returns
   ``Ok(EdsFile)``; ``EdsFile::warnings`` lists one entry per
   triggered quirk with the expected ``EdsWarning::kind``.
