Event-driven I/O dispatch
=========================

Test cases verifying the event-driven I/O dispatch sub-feature
(:need:`FEAT_0012`).

.. test:: Subscriber-triggered ingestion wakes the item
   :id: TEST_0107
   :status: implemented
   :verifies: REQ_0010

   **Goal.** A ``Subscriber<T>`` declared as a trigger via
   ``TriggerDeclarer::subscriber`` causes the executor to dispatch
   the item whenever the matching ``Publisher<T>`` sends a sample.

   **Fixture.** ``crates/taktora-executor/tests/run_loop.rs`` —
   ``subscriber_trigger_dispatches_task`` (lines 83-120). An
   inline-pool executor; a unique-named iceoryx2 channel; one
   item that declares the subscriber and increments a counter
   per dispatch. A background thread publishes five
   ``Tick(u64)`` samples 20 ms apart.

   **Steps.**

   1. Open the channel; build publisher and subscriber handles.
   2. Register the item; spawn the publisher thread.
   3. Call ``exec.run()``; the item calls ``stop_executor`` after
      three fires.

   **Expected outcome.** ``counter >= 3`` — subscriber-driven
   dispatch fires at least once per delivered sample (modulo the
   stop-after-three early exit).

.. test:: Publisher API send paths deliver to attached subscribers
   :id: TEST_0108
   :status: implemented
   :verifies: REQ_0011

   **Goal.** All three ``Publisher`` send paths (``send_copy``,
   ``loan_send``, ``loan``) are present, callable, and deliver the
   payload to an attached ``Subscriber``.

   **Fixture.** ``crates/taktora-executor/tests/channel.rs`` —
   three test functions covering the three send paths:
   ``publisher_send_notifies_subscriber_listener`` (lines 17-41,
   ``send_copy``), ``publisher_loan_zero_copy_round_trip``
   (lines 52-76, ``loan``), and ``publisher_loan_skip_returns_false``
   (lines 78-101, ``loan`` skip-publish path). Each opens a
   unique-named iceoryx2 channel, constructs a ``Publisher`` and
   ``Subscriber``, and round-trips a ``Msg(u64)`` payload.

   **Steps.**

   1. ``send_copy(Msg(42))`` — read back via
      ``Subscriber::take``; assert payload value.
   2. ``loan(|slot| { slot.write(Msg(99)); true })`` — read back
      via ``Subscriber::take``; assert payload value.
   3. ``loan(|_| false)`` — assert ``outcome.sent == false`` and
      ``Subscriber::take`` returns ``None``.

   **Expected outcome.** All three send paths compile, run, and
   deliver (or correctly skip) the configured payload.

.. test:: Publisher::loan round-trips without serialisation
   :id: TEST_0109
   :status: implemented
   :verifies: REQ_0012

   **Goal.** ``Publisher::loan(|slot: &mut MaybeUninit<T>| ...)``
   writes the payload directly into the iceoryx2 shared-memory
   slot, and ``Subscriber::take`` returns an
   ``iceoryx2::sample::Sample`` whose ``payload()`` is a borrowed
   view of the same bytes — no copy, no deserialisation.

   **Fixture.** ``crates/taktora-executor/tests/channel.rs`` —
   ``publisher_loan_zero_copy_round_trip`` (lines 52-76). Single
   iceoryx2 channel; one publisher, one subscriber.

   **Steps.**

   1. ``publisher.loan(|slot| { slot.write(Msg(99)); true })``.
   2. ``subscriber.take().unwrap().expect("payload")`` returns a
      ``Sample``.
   3. Assert ``sample.payload().0 == 99``.

   **Expected outcome.** The loan path delivers the producer's
   in-place write to the consumer as a borrowed view.

.. test:: NotifyOutcome surfaces listeners-notified count
   :id: TEST_0113
   :status: implemented
   :verifies: REQ_0013

   **Goal.** Every send path on ``Publisher`` returns a
   ``NotifyOutcome { sent, listeners_notified }`` whose
   ``listeners_notified`` field reports the number of attached
   subscribers actually notified — distinguishing back-pressure
   ("no listener attached") from delivery error.

   **Fixture.** ``crates/taktora-executor/tests/channel.rs`` —
   ``publisher_send_notifies_subscriber_listener`` (lines 17-41).
   One publisher, one subscriber; a single ``send_copy``.

   **Steps.**

   1. Build channel, publisher, and subscriber.
   2. Call ``publisher.send_copy(Msg(42))``.
   3. Read ``outcome.sent`` and ``outcome.listeners_notified``.

   **Expected outcome.** ``outcome.sent == true`` and
   ``outcome.listeners_notified == 1`` — the field surfaces
   delivery accounting as a non-error counter.
