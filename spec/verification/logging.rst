Logging — verification
======================

Test cases verifying the workspace-wide logging facade and its
default DLT backend (see :doc:`../requirements/logging`). Each
``test`` directive ``:verifies:`` one ``req`` parent from
:need:`REQ_0800` .. :need:`REQ_0814` and cites the path and line range
of the covering Rust test under ``crates/taktora-log/tests/`` or
``crates/taktora-log-dlt/tests/``. Mirrors the structure of
:doc:`bounded-alloc` for diff-friendly review.

The three remaining approved requirements in the chapter
(:need:`REQ_0813`, :need:`REQ_0815`, :need:`REQ_0816`) are deliberately
not promoted here — each one has an open spec-vs-implementation drift
flagged by the audit and needs a regenerate pass before its test can
land at ``status=implemented``.

----

Facade and backend-swap surface
-------------------------------

.. test:: Facade-only emission lands DLT bytes at the daemon
   :id: TEST_0800
   :status: implemented
   :verifies: REQ_0800

   **Goal.** Confirm that a caller using only the ``taktora-log``
   facade (no direct ``taktora-log-dlt`` dependency at the call site)
   produces DLT bytes that arrive at a co-located daemon, exercising
   the single-facade contract.

   **Fixture.** End-to-end test at
   ``crates/taktora-log-dlt/tests/via_facade.rs:22-63`` — a UDS-bound
   ``UnixListener`` stands in for ``dlt-daemon``; a ``DltBackend`` is
   built with ``AppId("TKEX")`` / ``CtxId("MAIN")`` and installed via
   ``taktora_log::init().with_sink(...)``.

   **Steps.**

   1. Bind a Unix-domain socket inside a temp dir.
   2. Build a ``DltBackend`` pointed at the socket and install it via
      ``taktora_log::init().with_sink(...).start()``.
   3. Emit via ``taktora_log::info!(target: "tk.endtoend", ...)`` —
      the facade re-export, not ``log::info!`` directly.
   4. Read the bytes off the socket and parse with
      ``dlt_core::parse::dlt_message``.

   **Expected outcome.** The parsed DLT message's application ID is
   ``"TKEX"`` — proving the facade routes through the active
   ``LogSink`` with no direct backend coupling at the call site.

.. test:: taktora-log re-exports compile as drop-in log macros
   :id: TEST_0801
   :status: implemented
   :verifies: REQ_0801

   **Goal.** Confirm that ``debug!`` / ``error!`` / ``info!`` /
   ``trace!`` / ``warn!`` are re-exported from ``taktora_log`` so a
   caller depending only on ``taktora-log`` can use them as drop-in
   equivalents of the upstream ``log::*`` macros.

   **Fixture.** Compile-only test at
   ``crates/taktora-log/tests/reexports.rs:6-15``.

   **Steps.**

   1. ``use taktora_log::{debug, error, info, trace, warn};``
   2. Invoke each macro once inside ``#[test]``.

   **Expected outcome.** The test binary compiles. Any missing
   re-export is a build break, so passing compilation is the
   verification.

.. test:: LogSink is object-safe
   :id: TEST_0802
   :status: implemented
   :verifies: REQ_0802

   **Goal.** Pin that ``LogSink`` stays object-safe so backends remain
   usable as ``Box<dyn LogSink>`` / ``Arc<dyn LogSink>`` for runtime
   selection — the backend-swap surface depends on this.

   **Fixture.** Compile-only test at
   ``crates/taktora-log/tests/log_sink_object_safe.rs:9-13``. Adapter
   forwarding (``LogSink`` → ``log::Log``) is additionally exercised
   in ``crates/taktora-log/tests/adapter.rs:25-38``.

   **Steps.**

   1. Declare ``fn assert_object_safe(_: &dyn LogSink) {}``.
   2. Reference the function so the compiler instantiates the
      ``&dyn LogSink`` coercion.

   **Expected outcome.** Compilation succeeds. If a future change
   adds a generic method or a ``Self: Sized`` bound that breaks object
   safety, this test fails to compile.

.. test:: Second taktora_log::init() returns AlreadyInitialized
   :id: TEST_0803
   :status: implemented
   :verifies: REQ_0803

   **Goal.** Confirm the one-shot init builder selects a backend on
   the first call and refuses to silently override on the second —
   the second call must return ``InitError::AlreadyInitialized``.

   **Fixture.**
   ``crates/taktora-log/tests/init_oneshot.rs:13-25``. The test
   isolates the global ``log::Log`` slot by running in its own
   integration-test binary.

   **Steps.**

   1. Call ``init().with_sink(Arc::new(Console::stderr_default())).start()``
      — must succeed.
   2. Call ``init().with_sink(...).start()`` a second time.

   **Expected outcome.** The second call returns
   ``Err(InitError::AlreadyInitialized)``.

.. test:: Pre-installed log::Log wins over taktora_log::init()
   :id: TEST_0804
   :status: implemented
   :verifies: REQ_0804

   **Goal.** Confirm that an integrator who registers their own
   ``log::Log`` via ``log::set_logger`` / ``log::set_boxed_logger``
   *before* invoking ``taktora_log::init()`` keeps it — init detects
   the pre-existing logger and returns
   ``InitError::PreExistingLogger`` rather than overriding.

   **Fixture.**
   ``crates/taktora-log/tests/init_respects_pre_existing.rs:33-44``.

   **Steps.**

   1. Install a ``PreExisting`` logger via
      ``log::set_boxed_logger``.
   2. Call ``init().with_sink(NeverCalled)...`` — the
      ``NeverCalled`` sink panics if hit.
   3. Emit ``log::info!("ping")``.

   **Expected outcome.** Init returns
   ``Err(InitError::PreExistingLogger)``; the ``PreExisting``
   logger receives ``"ping"``; ``NeverCalled`` is never invoked.

.. test:: tracing::* events reach the active LogSink
   :id: TEST_0805
   :status: implemented
   :verifies: REQ_0805

   **Goal.** Confirm that events emitted via ``tracing::info!`` /
   ``tracing::warn!`` / ``tracing::error!`` flow through the
   tracing-log bridge and are captured by the active ``LogSink`` —
   so existing ``taktora-executor-tracing`` emitters keep working
   without rewrite.

   **Fixture.**
   ``crates/taktora-log/tests/tracing_bridge.rs:26-43``. The bridge
   is activated passively by the ``tracing/log`` cargo feature
   declared in ``crates/taktora-log/Cargo.toml``.

   **Steps.**

   1. Install a ``Capture`` ``LogSink`` via
      ``init().with_sink(...).with_max_level(Trace).start()``.
   2. Emit ``tracing::info!(target: "tk.bridge.test", "hello from tracing")``.
   3. Read the captured records vector.

   **Expected outcome.** At least one captured record contains both
   ``"hello from tracing"`` and the target ``"tk.bridge.test"``.

----

DLT backend
-----------

.. test:: Encoder produces parseable AUTOSAR DLT R20-11 bytes
   :id: TEST_0806
   :status: implemented
   :verifies: REQ_0806

   **Goal.** Confirm the ``Encoder`` produces AUTOSAR Classic DLT
   R20-11 messages by encoding a synthetic ``log::Record`` and
   round-tripping it through ``dlt_core::parse::dlt_message`` — the
   parse-side ground truth for the spec.

   **Fixture.**
   ``crates/taktora-log-dlt/tests/encode_round_trip.rs:11-63``.
   ``Encoder::new(AppId("TKEX"), CtxId("MAIN"), "ECU1")``.

   **Steps.**

   1. Build a ``log::Record`` at level Info with target
      ``"tk.test"`` and ``args = "hello world 7"``.
   2. ``encoder.encode(&rec, /*timestamp_tenths_ms=*/ 1234)``.
   3. ``dlt_core::parse::dlt_message(&bytes, None, true)``.

   **Expected outcome.** Parse succeeds with no trailing bytes;
   ``ext.application_id == "TKEX"``, ``ext.context_id == "MAIN"``,
   ``message_type == Log(Info)``, and the verbose payload's leading
   argument is the string ``"hello world 7"``.

.. test:: UDS and TCP transports both connect and write
   :id: TEST_0807
   :status: implemented
   :verifies: REQ_0807

   **Goal.** Confirm that ``Transport::connect`` succeeds for both
   ``TransportConfig::Uds`` and ``TransportConfig::Tcp`` and that
   bytes written via ``Transport::write_all`` arrive at the listening
   peer.

   **Fixture.** Unix-domain-socket round-trip at
   ``crates/taktora-log-dlt/tests/transport.rs:6-30`` (gated
   ``#[cfg(unix)]``) and TCP round-trip at
   ``crates/taktora-log-dlt/tests/transport.rs:33-61``.

   **Steps.** (UDS)

   1. Bind a ``UnixListener`` on a temp-dir socket path.
   2. ``Transport::connect(&TransportConfig::Uds(sock))``.
   3. ``t.write_all(b"hello\\r\\n")`` then drop ``t``.

   **Steps.** (TCP)

   1. Bind a ``TcpListener`` on ``127.0.0.1:0``.
   2. ``Transport::connect(&TransportConfig::Tcp("127.0.0.1:{port}"))``.
   3. ``t.write_all(b"world")`` then drop ``t``.

   **Expected outcome.** Each server thread reads back the exact
   bytes sent. The backend never attempts to start, supervise, or
   reconfigure the daemon — only to connect to one provided by the
   integrator.

.. test:: AppId and CtxId enforce 4-character ASCII
   :id: TEST_0808
   :status: implemented
   :verifies: REQ_0808

   **Goal.** Confirm that the ``AppId`` and ``CtxId`` newtypes
   accept exactly four ASCII bytes, rejecting wrong-length and
   non-ASCII inputs at construction time.

   **Fixture.**
   ``crates/taktora-log-dlt/tests/ids.rs:4-37``. The builder additionally
   requires both fields before the backend can be constructed.

   **Steps.**

   1. ``AppId::new("TKEX")`` — Ok, ``as_str() == "TKEX"``.
   2. ``AppId::new("TKX")`` and ``AppId::new("TKXYZ")`` — must each
      return ``Err(IdError::WrongLength { .. })``.
   3. ``AppId::new("TKÄ")`` — must return ``Err(IdError::NonAscii)``.
   4. Same rules apply to ``CtxId``: ``CtxId::new("MAIN").is_ok()``
      and ``CtxId::new("MAI")`` returns ``WrongLength``.

   **Expected outcome.** All assertions hold; out-of-range IDs are
   rejected at the type boundary rather than reaching the encoder.

.. test:: log::kv pairs map to native DLT verbose arguments
   :id: TEST_0809
   :status: implemented
   :verifies: REQ_0809

   **Goal.** Confirm that structured ``log::kv`` pairs are emitted as
   per-key DLT verbose arguments with the native type chosen from the
   value's runtime type (``u32`` / ``f64`` / ``bool`` / ``&str``) and
   that the formatted message body is the leading argument.

   **Fixture.**
   ``crates/taktora-log-dlt/tests/kv_to_verbose.rs:7-72``. A
   synthetic ``log::kv::Source`` emits four typed pairs.

   **Steps.**

   1. Build a record with kv pairs ``count=7u32``, ``score=1.5f64``,
      ``ok=true``, ``name="alice"``.
   2. ``encoder.encode(&rec, 0)`` and parse with
      ``dlt_core::parse::dlt_message``.

   **Expected outcome.** Payload variant is ``Verbose``;
   ``args[0]`` is the formatted message string; ``args[1]`` is
   ``Value::U32(7)``; ``args[2]`` is ``Value::F64(~1.5)``; ``args[3]``
   is ``Value::Bool(1)``; ``args[4]`` is ``Value::StringVal("alice")``.

----

Runtime log-level control
-------------------------

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

----

Non-blocking hot path and offline buffering
-------------------------------------------

.. test:: Producer-side send never blocks the calling thread
   :id: TEST_0812
   :status: implemented
   :verifies: REQ_0812

   **Goal.** Confirm that pushing a record into the flusher's
   bounded channel completes in well under 5 ms regardless of what
   the flusher / daemon socket is doing — the producer never waits on
   ``connect``, ``write``, or any socket syscall.

   **Fixture.**
   ``crates/taktora-log-dlt/tests/flusher.rs:20-54`` (gated
   ``#[cfg(unix)]``). A ``UnixListener`` stands in for the daemon;
   the test asserts a wall-clock timing bound.

   **Steps.**

   1. Spawn the flusher with a UDS transport pointed at the listener
      and a 64-slot offline ring.
   2. Record ``t0 = Instant::now()`` and call
      ``tx.send(b"hello".to_vec())``.
   3. Assert ``t0.elapsed() < 5 ms``.
   4. Join the listener thread and confirm it read back ``"hello"``.

   **Expected outcome.** ``send`` returns in under 5 ms; the bytes
   arrive at the listener; the flusher shuts down cleanly. The
   producer hot path is bounded-queue push only — no socket I/O on
   the caller's thread.

.. test:: Offline ring buffers and drains FIFO across reconnect
   :id: TEST_0814
   :status: implemented
   :verifies: REQ_0814

   **Goal.** Confirm that records pushed into the ``OfflineRing``
   while the daemon socket is unavailable are drained in FIFO order
   on reconnect, and that mid-drain failure preserves the un-drained
   suffix in original order (no silent loss of un-sent records).

   **Fixture.** Ring-only FIFO and capacity tests at
   ``crates/taktora-log-dlt/tests/ring.rs:4-34``; flusher-level
   mid-drain failure rebuffers in FIFO order at
   ``crates/taktora-log-dlt/tests/flusher.rs:57-142`` (Unix-only).

   **Steps.** (ring-only)

   1. ``OfflineRing::with_capacity(3)``; push ``a``, ``b``; drain.
   2. ``OfflineRing::with_capacity(2)``; push ``a``, ``b``, ``c``;
      drain.

   **Steps.** (mid-drain failure)

   1. Pre-populate a capacity-16 ring with 5 large (256 KiB) records
      tagged ``'0'..'4'`` *before* the listener exists.
   2. Bind the listener, spawn the flusher; the listener accepts,
      reads one record, then ``shutdown(RDWR)`` on the connection.
   3. After 500 ms, ``handle.shutdown()`` and ``ring.drain_all()``.

   **Expected outcome.** Ring-only: drained order is ``[a, b]`` then
   ``[b, c]`` (oldest dropped, FIFO preserved). Mid-drain: exactly
   the 4 records ``'1'..'4'`` remain re-buffered, in order — proving
   the un-drained suffix is rebuffered intact when the write to the
   daemon fails.
