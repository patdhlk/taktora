.. _safety-tsc:

Technical Safety Concept — TSRs
===============================

Refinement of the AFSRs (see :doc:`fsc`) onto taktora's concrete crates.
TSRs are taktora's own commitments — not assumed. They take the ASIL of
their parent AFSR (B(D)).

Each TSR carries a **status** field describing today's implementation
state, with the convention:

* ``implemented`` — current code satisfies (references concrete FEAT/REQ/BB IDs).
* ``draft`` — requires extension to existing crate (gap analysis pending).

.. tsr:: Bounded allocator hard caps
   :id: TSR_0001
   :status: implemented
   :asil: B(D)
   :refines: AFSR_0003

   The bounded allocator (``taktora-bounded-alloc``) shall enforce hard
   compile-time caps on per-allocation size and total live blocks;
   allocation requests exceeding the cap shall return null per the
   ``core::alloc::GlobalAlloc`` contract.

   :Allocates to: ``taktora-bounded-alloc``
   :Today: Satisfied by FEAT_0040.

.. tsr:: Per-integrity-level allocation quotas
   :id: TSR_0002
   :status: implemented
   :asil: B(D)
   :refines: AFSR_0003

   The bounded allocator shall maintain partitioned quotas per
   integrity level, such that exhaustion of the QM-grade pool cannot
   deny allocation from the safety-critical pool.

   :Allocates to: ``taktora-bounded-alloc``
   :Today: Satisfied by ``PartitionedBoundedAllocator`` — independent
       safety-critical and quality-managed block pools with explicit
       per-level routing (``alloc_in``); QM-pool exhaustion cannot deny
       the SC pool. Verified by ``TEST_0130``.

.. tsr:: Integrity-level declaration and process isolation
   :id: TSR_0003
   :status: implemented
   :asil: B(D)
   :refines: AFSR_0001

   Each ``ExecutableItem`` registration shall declare an integrity
   level (``SafetyCritical`` | ``QualityManaged``); the executor shall
   reject in-process co-hosting of mixed integrity levels and require
   QM-grade items to run in a separate OS process.

   :Allocates to: ``taktora-executor``
   :Today: Satisfied by ``IntegrityLevel`` + ``ExecutableItem::integrity_level``
       + ``ExecutorBuilder::integrity_level``; ``add`` / ``add_chain`` /
       ``add_graph`` reject mixed levels with ``ExecutorError::MixedIntegrity``.
       Verified by ``TEST_0131``.

.. tsr:: Missed-deadline detection within one cycle
   :id: TSR_0004
   :status: implemented
   :asil: B(D)
   :refines: AFSR_0004

   Missed-deadline detection shall fire within one cycle of the
   configured interval and propagate via ``ExecutionMonitor``.

   :Allocates to: ``taktora-executor``
   :Today: Satisfied by the executor's existing deadline monitor.

.. tsr:: Compile-time channel directionality
   :id: TSR_0005
   :status: implemented
   :asil: B(D)
   :refines: AFSR_0002

   The ``ChannelWriter`` / ``ChannelReader`` types shall enforce
   direction at compile time via the Rust type system; runtime
   construction shall not be able to forge a writer from a reader
   handle.

   :Allocates to: ``taktora-connector-host``, ``taktora-connector-core``
   :Today: Satisfied by BB_0001, BB_0005.

.. tsr:: Bounded health-event latency
   :id: TSR_0006
   :status: approved
   :asil: B(D)
   :refines: AFSR_0004

   ``ConnectorHealth`` events shall be emitted within FTTI/2 (at most
   50 ms) of a connector state transition between the
   ``Up``, ``Connecting``, ``Degraded``, and ``Down`` states tracked
   by ``ConnectorHealth`` (see
   ``crates/taktora-connector-core/src/health.rs``).

   :Allocates to: ``taktora-connector-host``, ``taktora-connector-zenoh``
   :Today: Health-state emission lives in
       ``taktora-connector-zenoh`` (REQ_0442 and friends), but no
       regression test currently asserts the 50 ms FTTI/2 upper bound
       on transition latency. Demoted to ``approved`` until that
       bound is measured and a covering ``test::`` is added.

.. tsr:: Single-publisher iceoryx2 topology for safety-critical channels
   :id: TSR_0007
   :status: implemented
   :asil: B(D)
   :refines: AFSR_0002

   iceoryx2 services backing safety-critical channels shall be
   configured with single-publisher topology; the publisher process
   holds the only write capability over the underlying shared-memory
   segment.

   :Allocates to: ``taktora-connector-transport-iox``
   :Today: The transport-iox factory now sets single-publisher
       (``max_publishers(1)``) and the buffer/history QoS explicitly, and
       a static ``ChannelSpec`` topology is created once at init with
       undeclared services rejected. Verified by ``TEST_0132``.

.. tsr:: Envelope sequence + CRC integrity
   :id: TSR_0008
   :status: implemented
   :asil: B(D)
   :refines: AFSR_0002, AFSR_0004

   The ``ConnectorEnvelope`` POD wire format shall carry a sequence
   counter and a CRC over header + payload; CRC mismatch on read
   shall raise a ``HealthEvent`` and discard the frame without
   surfacing it to the reader.

   :Allocates to: ``taktora-connector-transport-iox``
   :Today: Satisfied by the ``crc32`` envelope header field (wire
       version 2): a CRC over header + payload is computed on send and
       verified on receive; a mismatch drops the frame and raises a
       ``HealthEvent(Degraded)`` without surfacing it. Verified by
       ``TEST_0133``.

.. tsr:: Cross-process hosting mode
   :id: TSR_0009
   :status: implemented
   :asil: B(D)
   :refines: AFSR_0001, AFSR_0002

   Taktora shall provide a hosting mode in which safety-critical items
   and QM-grade items run in distinct OS processes communicating
   exclusively through iceoryx2 shared-memory channels with per-process
   read/write capability.

   :Allocates to: ``taktora-executor``, ``taktora-connector-host``
   :Today: Satisfied by per-process ``IntegrityLevel``-pinned executors
       that communicate only over iceoryx2 shared memory — see the
       ``integrity-cross-process`` two-process example. Verified by
       ``TEST_0134``.

.. tsr:: Heartbeat for Element B monitor
   :id: TSR_0010
   :status: implemented
   :asil: B(D)
   :refines: AFSR_0004

   The safety-critical executor process shall emit a heartbeat
   ``HealthEvent`` at a period at most FTTI/2 (50 ms) to support the
   integrator's diverse monitor (Element B per :doc:`decomposition`).

   :Allocates to: ``taktora-executor``, ``taktora-connector-host``
   :Today: Satisfied by ``ExecutorBuilder::heartbeat`` +
       ``Observer::on_heartbeat`` (the dispatch-loop wait is bounded by
       the heartbeat deadline) and the connector-host
       ``HeartbeatHealthBridge`` that forwards ticks onto the
       ``HealthEvent`` path. Verified by ``TEST_0135``.

.. tsr:: Cold-start integrity-verification admission gate
   :id: TSR_0011
   :status: implemented
   :asil: B(D)
   :refines: AFSR_0005

   The executor shall verify that the spatial-isolation context is
   intact before admitting a safety-critical item into the runnable set
   on each cold start; a failed verification shall refuse admission and
   surface a fault without dispatching any item.

   :Allocates to: ``taktora-executor``
   :Today: Satisfied by ``ExecutorBuilder::admission_check`` +
       ``AdmissionContext`` / ``AdmissionOutcome``: the ordered
       verify → admit → ``RUNNING`` startup runs before dispatch, and a
       rejection yields ``ExecutorError::AdmissionRejected`` +
       ``Observer::on_admission_rejected`` with nothing dispatched.
       Verified by ``TEST_0136``.

TSR coverage summary
--------------------

.. needtable::
   :types: tsr
   :columns: id, title, status, refines
   :show_filters:

* 10 ``implemented`` — TSR_0001..TSR_0005, TSR_0007..TSR_0011.
* 1 ``approved`` — TSR_0006 (pending a measured FTTI/2 latency test).

**AFSR coverage.** This concept refines AFSR_0001..AFSR_0005 onto 11
TSRs. AFSR_0005 (startup integrity verification) is refined by TSR_0011,
the cold-start admission gate — the admission-time companion to the
TSR_0003 process-isolation invariant.

Covering ``test::`` cases for the context-based isolation work item live
in :doc:`verification`.
