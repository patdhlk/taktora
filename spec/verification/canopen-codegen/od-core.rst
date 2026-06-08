OD-core unit tests
==================

Per-crate, no I/O beyond synthetic inputs, parallel-safe. Live
under ``crates/fieldbus-od-core/tests/``.

.. test:: Identity, DictEntry, PdoEntry round-trip
   :id: TEST_0600
   :status: open
   :verifies: REQ_0702

   Unit tests construct each public type with synthetic values,
   clone, and compare for structural equality. No I/O. Confirms the
   public surface compiles and supports the trait derives expected
   by :need:`REQ_0702`.

.. test:: fieldbus-od-core has no transport deps
   :id: TEST_0601
   :status: open
   :verifies: REQ_0700

   CI shell check: ``cargo tree -p fieldbus-od-core --no-default-features``
   shall not list ``ethercrab``, ``socketcan``, ``taktora-connector-can``,
   ``taktora-connector-ethercat``, or any I/O-bearing crate in the
   resolved graph. Fails the CI job on any match.

.. test:: fieldbus-od-core compiles under no_std + alloc
   :id: TEST_0602
   :status: open
   :verifies: REQ_0701

   Compile-only test: a small bin target inside
   ``crates/fieldbus-od-core/tests/no_std/`` declares ``#![no_std]``,
   uses ``alloc::vec::Vec``, constructs each type. Compiles with
   ``--no-default-features``; the test passes if compilation
   succeeds.

.. test:: ethercat-esi re-exports lifted types
   :id: TEST_0603
   :status: open
   :verifies: REQ_0703, REQ_0704

   Compile-only test: a test crate under
   ``crates/ethercat-esi/tests/reexport/`` writes
   ``use ethercat_esi::{Identity, DictEntry, PdoEntry, PdoMap};``
   and constructs each. Compiles if and only if the re-export
   façade is in place.
