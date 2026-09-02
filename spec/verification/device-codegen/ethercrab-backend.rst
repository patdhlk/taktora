ethercrab backend snapshot tests
=================================

Live under ``crates/ethercat-esi-codegen-ethercrab/tests/``.

.. test:: EL3001 backend output snapshot
   :id: TEST_0420
   :status: implemented
   :verifies: REQ_0521, REQ_0522

   Run parse → codegen → backend → prettyplease on the canonical
   ``EL3001`` ESI fixture. Compare the formatted output against a
   committed ``snapshots/el3001.rs`` golden file using
   ``insta::assert_snapshot!``. Reviewer regenerates the golden
   when intentional changes land; CI fails on unintentional
   churn. The ``<Dev>OpMode`` enum shape (:need:`REQ_0523`,
   :need:`REQ_0524`) is verified directly by :need:`TEST_0870`.

.. test:: EL7047 OpMode enum, exact lengths, pdo_assignment, round-trip
   :id: TEST_0870
   :status: implemented
   :verifies: REQ_0523, REQ_0524, REQ_0528

   Tests in
   ``crates/taktora-ethercat-esi-codegen-ethercrab-tests/tests/op_mode.rs``
   run the EL7047 ESI through parse → codegen → backend and assert the
   joint per-device ``OpMode`` enum: all nine operating modes are
   emitted as variants (:need:`REQ_0523`); each variant's
   ``{ inputs, outputs }`` payload yields the exact per-mode
   ``input_len`` / ``output_len`` and a decode/encode round-trip is
   lossless for the active mode (:need:`REQ_0524`); and
   ``pdo_assignment()`` returns the expected Rx (``0x1C12`` / SM2) and
   Tx (``0x1C13`` / SM3) ``u16`` index lists per active mode
   (:need:`REQ_0528`).

.. test:: Generated registry covers every emitted device
   :id: TEST_0421
   :status: implemented
   :verifies: REQ_0525

   For an input set with N devices, the generated module's
   ``registry!()`` expansion contains exactly N entries mapping
   ``SubDeviceIdentity`` → factory closure. White-box test
   parses the generated output and counts entries.

.. test:: Generated module compiles under no_std + alloc
   :id: TEST_0422
   :status: open
   :verifies: REQ_0526

   A test crate at
   ``crates/ethercat-esi-codegen-ethercrab/tests/no_std_consumer/``
   has ``#![no_std]`` and ``extern crate alloc;``, ``include!``s
   the generated module from a fixed input set, and compiles
   successfully. Catches any accidental ``std::`` qualified path
   in the backend's emit code.

.. test:: Backend is the sole ethercrab consumer in the toolchain
   :id: TEST_0423
   :status: open
   :verifies: REQ_0520

   CI shell check: ``cargo tree`` invocations for
   ``ethercat-esi``, ``ethercat-esi-codegen``,
   ``ethercat-esi-build``, ``ethercat-esi-cli``, and
   ``ethercat-esi-verify`` must none of them list ``ethercrab``
   in the dependency graph. ``ethercat-esi-codegen-ethercrab``
   and ``ethercat-esi-rt`` are the only crates where
   ``ethercrab`` is allowed.

.. test:: Object-dictionary emission gated by feature flag
   :id: TEST_0424
   :status: open
   :verifies: REQ_0533

   Build the ``no_std_consumer`` test crate twice: once without
   features (the generated module's OD table is empty / absent;
   no symbol named ``OD`` exists) and once with
   ``--features object-dictionary`` (the OD ``static`` exists
   and has the expected entry count for the input set). Compares
   the two binaries' rodata sections — the no-feature build is
   smaller by an amount approximating the OD table size.
