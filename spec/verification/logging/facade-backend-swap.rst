Facade and backend-swap surface
===============================

Test cases for the ``taktora-log`` facade (:need:`FEAT_0071`) and the
backend-swap surface (:need:`FEAT_0073`) — single-facade routing,
re-exports, object safety, one-shot init, pre-existing-logger
precedence, and the tracing bridge.

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
