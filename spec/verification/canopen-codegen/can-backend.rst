taktora backend snapshot tests
==============================

Live under ``crates/canopen-eds-codegen-taktora/tests/``.

.. test:: EPOS4 backend output snapshot
   :id: TEST_0630
   :status: open
   :verifies: REQ_0741, REQ_0742, REQ_0743, REQ_0744

   Run parse → codegen → backend → prettyplease on the canonical
   ``EPOS4`` EDS fixture. Compare the formatted output against a
   committed ``snapshots/epos4.rs`` golden file using
   ``insta::assert_snapshot!``. Reviewer regenerates the golden
   when intentional changes land; CI fails on unintentional
   churn.

.. test:: Generated registry covers every emitted device
   :id: TEST_0631
   :status: open
   :verifies: REQ_0745

   For an input set with N EDS files, the generated module's
   ``registry!()`` expansion contains exactly N entries mapping
   ``Identity → factory closure``. White-box test parses the
   generated output and counts entries.

.. test:: Generated module compiles under no_std + alloc
   :id: TEST_0632
   :status: open
   :verifies: REQ_0748

   A test crate at
   ``crates/canopen-eds-codegen-taktora/tests/no_std_consumer/``
   has ``#![no_std]`` and ``extern crate alloc;``, ``include!``s
   the generated module from a fixed input set, and compiles
   successfully. Catches any accidental ``std::``-qualified path
   in the backend's emit code.

.. test:: Backend is the sole canopen-eds-rt consumer in the toolchain
   :id: TEST_0633
   :status: open
   :verifies: REQ_0740

   CI shell check: ``cargo tree`` invocations for ``canopen-eds``,
   ``canopen-eds-codegen``, ``canopen-eds-build``,
   ``canopen-eds-cli``, and ``canopen-eds-verify`` must none of
   them list ``canopen-eds-rt`` in the dependency graph.
   ``canopen-eds-codegen-taktora`` is the only crate where
   ``canopen-eds-rt`` is allowed.

.. test:: Object-dictionary emission gated by feature flag
   :id: TEST_0634
   :status: open
   :verifies: REQ_0747

   Build the ``no_std_consumer`` test crate twice: once without
   features (the generated module exposes no ``OD`` symbol) and
   once with ``--features object-dictionary`` (the OD ``static``
   exists with the expected entry count for the input set).
   Compares the two binaries' rodata sections — the no-feature
   build is smaller by an amount approximating the OD table size.

.. test:: Dummy entries skipped in PDO struct fields
   :id: TEST_0635
   :status: open
   :verifies: REQ_0744

   Fixture EDS declares a PDO mapping containing one real
   ``0x6040:00`` entry, one ``Dummy32`` entry, and one real
   ``0x607A:00`` entry. The generated PDO payload struct
   contains exactly two fields (the two real entries). The
   generated ``encode`` body writes zeros in the bit range
   covered by the dummy; the generated ``decode`` body skips it.

.. test:: Bring-up SDO writes emitted from EDS
   :id: TEST_0636
   :status: open
   :verifies: REQ_0746

   Snapshot test on the generated ``impl CanOpenConfigurable``
   body for the EPOS4 fixture asserts the expected sequence of
   ``sdo.write(index, sub, value)`` calls: PDO comm params first,
   then PDO mapping params, then optional NMT start. Sequence and
   target indices match the EDS values exactly.
