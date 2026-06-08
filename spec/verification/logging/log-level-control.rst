Runtime log-level control
=========================

Test cases for runtime per-context log-level control
(:need:`FEAT_0075`) — applying DLT control messages and the production
INFO default.

.. test:: Set-Log-Level and Set-Default control messages apply
   :id: TEST_0810
   :status: implemented
   :verifies: REQ_0810

   **Goal.** Confirm that DLT control messages from the daemon
   (Set-Log-Level for one Context ID; Set-Default-Log-Level for the
   global default) update the per-context atomic level table and are
   observable via ``LevelTable::current``.

   **Fixture.**
   ``crates/taktora-log-dlt/tests/control.rs:7-35``. The flusher's
   receive-half wiring lives in ``src/flusher.rs`` and is exercised
   indirectly via the apply path; parser unit tests live in
   ``crates/taktora-log-dlt/src/control.rs``.

   **Steps.**

   1. ``ControlMessage::SetLogLevel { ctx: CtxId("MAIN"), level: Debug }.apply(&table)``.
   2. ``ControlMessage::SetDefaultLogLevel(Warn).apply(&table)``.

   **Expected outcome.** After (1), ``table.current(CtxId("MAIN"))``
   reads ``Debug``. After (2), ``table.current(CtxId("NEW1"))``
   (a context never written to explicitly) reads ``Warn``.

.. test:: Default level is INFO until overridden
   :id: TEST_0811
   :status: implemented
   :verifies: REQ_0811

   **Goal.** Confirm that ``LevelTable::new(Level::Info)`` (the
   production default selected by ``Builder::with_max_level``)
   resolves to ``Info`` for any Context ID before any runtime
   override, so production builds never default to ``Debug``.

   **Fixture.**
   ``crates/taktora-log-dlt/tests/level_table.rs:4-9``.

   **Steps.**

   1. ``let table = LevelTable::new(log::Level::Info);``
   2. ``table.current(&CtxId::new("MAIN").unwrap())``.

   **Expected outcome.** Returns ``log::Level::Info``. Reaching
   ``Debug`` / ``Trace`` requires an explicit
   :need:`REQ_0810` control message or a builder override.
