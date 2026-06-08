DLT backend
===========

Test cases for the ``taktora-log-dlt`` backend (:need:`FEAT_0072`) and
its structured-field mapping (:need:`FEAT_0074`) — encoding,
transports, ID validation, and ``log::kv`` → DLT verbose arguments.

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
