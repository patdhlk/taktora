Codegen / IR tests
==================

Per-crate, snapshot-based. Live under
``crates/ethercat-esi-codegen/tests/``.

.. test:: Name sanitisation handles ESI naming edge cases
   :id: TEST_0410
   :status: implemented
   :verifies: REQ_0511

   Parameterised test asserting the sanitisation map for a fixed
   table of ESI product names → Rust idents: ``EL3001-0000`` →
   ``EL3001_0000``, ``EL3204 with spaces`` → ``EL3204_with_spaces``,
   leading-digit ``1234-Module`` → ``_1234_Module``, empty / pure
   punctuation → error.

.. test:: Revision collision produces distinct idents
   :id: TEST_0411
   :status: implemented
   :verifies: REQ_0512

   Synthetic input set containing two devices with identical
   product name but different revisions (``0x00100000`` and
   ``0x00110000``). The generated module shall contain
   ``EL3204_REV0010`` and ``EL3204_REV0011`` (or equivalent
   deterministic suffixing). Reordering the input file list shall
   produce the same idents in the same definitions (assert via
   string comparison of generated source).

.. test:: PDO entry dedup collapses structurally identical layouts
   :id: TEST_0412
   :status: open
   :verifies: REQ_0513

   Two synthetic devices whose RxPDO entries have identical
   ``(field order, bit lengths, data types)`` produce a single
   shared PDO entry struct in the generated module; the two
   device structs reference it by name. Asserted by counting
   distinct ``PdoEntry``-bearing struct definitions in the
   ``TokenStream`` (expect 1, not 2).

.. test:: TokenStream emission, not string formatting
   :id: TEST_0413
   :status: open
   :verifies: REQ_0514

   White-box test inside ``ethercat-esi-codegen`` asserts that
   ``emit_device`` and ``emit_module_root`` return
   ``TokenStream`` values directly (compile-time check via the
   return type). A complementary lint forbids ``format!`` /
   ``write!`` / ``writeln!`` invocations within the codegen
   crate's source.
