Runtime trait surface tests
===========================

Live under ``crates/canopen-eds-rt/tests/``.

.. test:: CanOpenDevice trait shape compiles for a hand-written device
   :id: TEST_0640
   :status: open
   :verifies: REQ_0750, REQ_0754, REQ_0755

   Hand-written test impl of ``CanOpenDevice`` for a minimal
   ``MockDevice`` validates the trait surface compiles end-to-end.
   Asserts ``IDENTITY`` is reachable, ``node_id`` / ``set_node_id``
   round-trip, ``nmt_state`` / ``set_nmt_state`` round-trip,
   ``on_rpdo`` is callable on ``&mut self`` with ``idx: u8`` in
   ``0..=3``, ``drain_tpdos`` is callable with a ``TpdoSink``, and
   each error path returns the expected ``CanOpenError`` variant
   (``PdoLenMismatch``, ``NmtStateViolation``, ``AbortCode``,
   ``TransportFailed``).

.. test:: CanOpenConfigurable async trait shape compiles
   :id: TEST_0641
   :status: open
   :verifies: REQ_0751

   Compile-only test: a mock device implements
   ``CanOpenConfigurable`` and an ``async fn configure`` body
   driving a mock ``SdoClient``. The test passes if compilation
   succeeds; catches any trait-method-async surface drift.

.. test:: canopen-eds-rt is the trait home, not taktora-internal
   :id: TEST_0642
   :status: open
   :verifies: REQ_0752

   CI shell check: ``rg "trait CanOpenDevice"`` across the
   workspace matches exactly one source location, inside
   ``crates/canopen-eds-rt/src/``. Same check for
   ``trait CanOpenConfigurable``.

.. test:: PdoOut payload uses heapless::Vec<u8, 8>
   :id: TEST_0643
   :status: open
   :verifies: REQ_0753

   Compile-time check: ``static_assertions::assert_type_eq_all!``
   on ``PdoOut::payload``'s type vs ``heapless::Vec<u8, 8>``. A
   change to the buffer type (e.g. lifting to a const generic for
   CAN-FD) requires this test to be updated deliberately rather
   than passing accidentally.

.. test:: RPDO rejected outside Operational state
   :id: TEST_0644
   :status: open
   :verifies: REQ_0756

   Integration test against a generated device fixture: set
   ``nmt_state(NmtState::PreOperational)``, call ``on_rpdo`` with
   a valid frame, assert the returned error is
   ``CanOpenError::NmtStateViolation``. Switch to
   ``NmtState::Operational`` and confirm the same frame succeeds.
