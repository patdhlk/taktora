Non-blocking hot path and offline buffering
===========================================

Test cases for the non-blocking producer hot path and the offline ring
buffer with reconnect drain (:need:`FEAT_0076`).

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
