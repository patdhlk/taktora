.. _safety-verification:

Verification — context-based isolation
======================================================

Covering ``test::`` cases for the context-based integrity-isolation and
deterministic-lifecycle work item (:doc:`tsc`). Each case links to the
technical safety requirement it exercises; the realising Rust tests live
in the crates named below.

.. test:: Per-integrity-level allocation quota isolation
   :id: TEST_0130
   :status: implemented
   :verifies: TSR_0002

   Exhausting the quality-managed pool of a
   ``PartitionedBoundedAllocator`` returns null for further QM requests
   while safety-critical allocations still succeed (and vice-versa),
   proving the pools cannot starve one another. Realised by
   ``taktora-bounded-alloc`` in ``tests/quota_isolation.rs``.

.. test:: In-process mixed-integrity rejection
   :id: TEST_0131
   :status: implemented
   :verifies: TSR_0003

   An executor pinned to one ``IntegrityLevel`` rejects ``add`` /
   ``add_chain`` / ``add_graph`` of any item declaring a different level
   with ``ExecutorError::MixedIntegrity``, while an unpinned executor
   accepts mixed levels unchanged. Realised by ``taktora-executor`` in
   ``tests/integrity_rejection.rs``.

.. test:: Explicit single-publisher and static channel topology
   :id: TEST_0132
   :status: implemented
   :verifies: TSR_0007

   ``ServiceFactory::create_all`` creates every declared ``ChannelSpec``
   service once with explicit QoS; a second publisher on a
   single-publisher service is refused, and opening an undeclared
   service after ``create_all`` is rejected. Realised by
   ``taktora-connector-transport-iox`` in ``tests/static_topology.rs``.

.. test:: Envelope CRC and sequence-gap integrity
   :id: TEST_0133
   :status: implemented
   :verifies: TSR_0008

   A corrupted ``ConnectorEnvelope`` fails ``verify_crc`` and is dropped
   on receive (``try_recv`` yields nothing) with a
   ``HealthEvent(Degraded)`` raised; sequence gaps are detected and
   surfaced likewise. Realised by
   ``taktora-connector-transport-iox`` in ``tests/envelope_integrity.rs``.

.. test:: Two-process safety-critical / quality-managed hosting
   :id: TEST_0134
   :status: implemented
   :verifies: TSR_0009

   Two distinct OS processes — one executor pinned ``SafetyCritical``
   (write capability), one ``QualityManaged`` (read capability) —
   exchange data exclusively over an iceoryx2 shared-memory channel and
   both exit cleanly. Realised by the ``integrity-cross-process``
   example in ``tests/two_process.rs``.

.. test:: Heartbeat emitted within the FTTI/2 bound
   :id: TEST_0135
   :status: implemented
   :verifies: TSR_0010

   With a configured heartbeat period and no other triggers, the
   executor emits ``HeartbeatTick`` observations whose inter-tick gap
   stays within a bounded multiple of the period, and the sequence is
   strictly monotonic. Realised by ``taktora-executor`` in
   ``tests/heartbeat.rs`` and the connector-host bridge tests.

.. test:: Cold-start admission gate refuses on failed verification
   :id: TEST_0136
   :status: implemented
   :verifies: TSR_0011

   An admission check returning ``Rejected`` causes ``run`` to fail with
   ``ExecutorError::AdmissionRejected`` before any item dispatches
   (the execute counter stays zero) and fires
   ``Observer::on_admission_rejected``; an ``Admitted`` outcome proceeds
   normally. Realised by ``taktora-executor`` in ``tests/admission.rs``.
