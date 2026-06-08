Codegen / IR tests
==================

Per-crate, snapshot-based. Live under
``crates/canopen-eds-codegen/tests/``.

.. test:: Name sanitisation handles EDS naming edge cases
   :id: TEST_0620
   :status: open
   :verifies: REQ_0731

   Parameterised test asserting the sanitisation map for a fixed
   table of EDS ``ProductName`` values → Rust idents: ``EPOS4 50/5``
   → ``EPOS4_50_5``, ``Module-V2`` → ``Module_V2``,
   ``1234 Drive`` → ``_1234_Drive``, empty / pure punctuation →
   error.

.. test:: Revision collision produces distinct idents
   :id: TEST_0621
   :status: open
   :verifies: REQ_0732

   Synthetic input set with two EDS files sharing ``ProductName``
   but differing in ``RevisionNumber`` (``0x01400000`` and
   ``0x01410000``). The generated module shall contain two
   distinct device structs with deterministic suffixes (e.g.
   ``EPOS4_REV0140`` and ``EPOS4_REV0141``). Reordering the input
   file list shall produce the same idents (string-compare of
   generated source).

.. test:: PDO entry dedup collapses structurally identical layouts
   :id: TEST_0622
   :status: open
   :verifies: REQ_0733

   Two synthetic devices whose RPDO entries have identical
   ``(bit_len, data_type)`` tuple lists but different field names
   produce a single shared PDO entry struct in the generated
   module. Asserted by counting distinct PDO-entry struct
   definitions in the ``TokenStream`` (expect 1, not 2).

.. test:: TokenStream emission, not string formatting
   :id: TEST_0623
   :status: open
   :verifies: REQ_0730, REQ_0734

   White-box test inside ``canopen-eds-codegen`` asserts that
   ``emit_device`` and ``emit_module_root`` return ``TokenStream``
   values directly (compile-time check via return type). A
   complementary lint forbids ``format!`` / ``write!`` /
   ``writeln!`` invocations within the codegen crate's source.

.. test:: One EDS file equals one device
   :id: TEST_0624
   :status: open
   :verifies: REQ_0735

   For an N-file fixture set, the call
   ``generate(&files, &TaktoraBackend::default())`` produces a
   module with exactly N distinct device-struct definitions and
   one shared registry table.
