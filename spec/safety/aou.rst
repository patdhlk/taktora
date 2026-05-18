.. _safety-aou:

Assumptions of Use
==================

The SEooC contract with the integrator. Each AoU is a claim taktora
*makes* about the integrator's environment or process. The integrator
MUST validate every AoU before claiming any ASIL for a taktora-hosted
item.

.. aou:: Diverse Element B monitor at ASIL B(D)
   :id: AOU_0001
   :status: open

   The integrator supplies a diverse, independent **Element B monitor**
   of equivalent ASIL B(D) capability that observes taktora's outputs and
   forces safe state on detected omission or value failure.

   :Validates: Decomposition (:doc:`decomposition`)

.. aou:: Independence between Element A and Element B
   :id: AOU_0002
   :status: open

   Element A (taktora) and Element B (monitor) run on independent CPU
   cores or independent SoCs, with independent power and clock
   domains where feasible.

   :Validates: Independence per ISO 26262-9 §5.4.4

.. aou:: Heartbeat receiver and safe-state path
   :id: AOU_0003
   :status: open

   The integrator implements the **receiver side** of taktora's heartbeat
   protocol and the safe-state forcing path with reaction time at most
   ``FTTI − taktora's emission period`` (at most 50 ms given FTTI=100 ms
   and heartbeat period ≤ FTTI/2).

   :Validates: :need:`TSR_0010`

.. aou:: Host OS provides MMU isolation and deterministic scheduling
   :id: AOU_0004
   :status: open

   The host OS provides MMU-enforced address-space isolation between
   processes and a deterministic scheduling discipline (real-time class
   or deadline-based scheduling).

   :Validates: :need:`TSR_0003`, :need:`TSR_0009`

.. aou:: Real-time scheduling and CPU pinning for SC process
   :id: AOU_0005
   :status: open

   The integrator pins the SC process to dedicated CPU core(s) and
   configures it under SCHED_FIFO or SCHED_DEADLINE; QM processes are
   excluded from those cores.

   :Validates: Temporal FFI

.. aou:: Integrator confirms HARA inputs and FTTI
   :id: AOU_0006
   :status: open

   The integrator validates that the assumed hazards (:need:`AHZ_0001`,
   :need:`AHZ_0002`) and assumed safety goals (:need:`ASG_0001`,
   :need:`ASG_0002`) match the result of their own HARA. The FTTI of
   100 ms is confirmed or replaced.

   :Validates: Whole concept

.. aou:: Integrator owns safe-state semantics
   :id: AOU_0007
   :status: open

   The integrator's application logic enters a defined safe state on
   receipt of ``HealthEvent::Faulted`` or on absence of expected channel
   data within deadline. Taktora raises faults; it does not define what
   safe state means for any particular application.

   :Validates: :need:`AFSR_0004`

.. aou:: Integrator unsafe-Rust discipline
   :id: AOU_0008
   :status: open

   The integrator's own ``ExecutableItem`` implementations use
   ``unsafe`` Rust only in ways that do not violate spatial isolation
   invariants — no aliasing of channel handles, no escape of writable
   references across integrity-level boundaries, no shared mutable
   state outside iceoryx2 channels.

   :Validates: Spatial FFI

.. aou:: Lower-stack qualification at ASIL B(D)
   :id: AOU_0009
   :status: open

   The integrator confirms that the host OS kernel, libc, iceoryx2
   runtime, and Rust toolchain are qualified to at least ASIL B(D).
   Taktora does not qualify these — they sit below taktora in the stack.

   :Validates: Whole stack

Logging (taktora-log / taktora-log-dlt)
---------------------------------------

These AoUs cover the workspace logging surface (see
:doc:`../requirements/logging` and :doc:`../architecture/logging`).
Logging is QM (per :need:`CON_0027`); every safety-relevant property of
the log stream depends on the integrator's deployment, so taktora
carries the responsibility as AoUs rather than as TSRs.

.. aou:: Integrator provides a DLT daemon
   :id: AOU_0010
   :status: open

   The integrator provides a COVESA ``dlt-daemon`` (or compatible)
   listening on the Unix-domain socket or TCP endpoint configured at
   ``taktora-log-dlt`` init. ``taktora-log-dlt`` does not start,
   supervise, restart, or reconfigure the daemon.

   :Validates: :need:`REQ_0807`

.. aou:: Integrator owns FFI freedom-from-interference
   :id: AOU_0011
   :status: open

   If the integrator swaps the pure-Rust DLT backend for any backend
   that crosses an FFI boundary — including ``libdlt`` adapters
   (``dlt_log``, ``dlt-rs``, ``tracing-dlt``) or vendor logger SDKs
   — the integrator owns the freedom-from-interference argument
   (separate process, memory partitioning, supervised lifecycle).
   taktora's spec only covers the pure-Rust DLT backend at
   :need:`BB_0091`.

   :Validates: :need:`CON_0025`, :need:`FEAT_0073`

.. aou:: Safety-relevant hot paths do not log
   :id: AOU_0012
   :status: open

   Integrator code on safety-relevant hot paths (ASIL-rated loops,
   the executor's deadline-critical sections, the
   ``HealthEvent::Faulted`` emit path) shall not log inside the
   tightest loops. ``taktora-log`` is best-effort, lossy under
   overload (per :need:`REQ_0815`), and not certified. Logging from
   a safety path is acceptable only when the path can absorb a
   dropped record without changing its safety behaviour.

   :Validates: :need:`CON_0027`, :need:`QG_0020`

.. aou:: DLT App ID uniqueness on the ECU
   :id: AOU_0013
   :status: open

   The integrator ensures DLT App IDs are unique across all processes
   on the same ECU. The 4-character DLT App ID namespace is flat;
   colliding IDs make DLT Viewer / dlt-tui filtering ambiguous.
   taktora reserves the ``TK*`` prefix for its own crates (per
   :need:`REQ_0808`); integrators shall pick non-``TK*`` IDs for
   their own applications.

   :Validates: :need:`REQ_0808`

.. aou:: Integrator sizes ring capacity and runtime log level
   :id: AOU_0014
   :status: open

   The integrator chooses the bounded ring capacity (per
   :need:`REQ_0814`), the runtime production log level (per
   :need:`REQ_0811`), and any non-default reconnect-backoff
   parameters, based on the ECU's memory and bandwidth budget.
   taktora ships safe defaults but does not size them for any
   specific ECU. The default ring capacity should be re-evaluated
   for high-volume integrations (e.g. ADAS perception pipelines)
   where overflow under sustained daemon outage would otherwise
   drop forensically important records.

   :Validates: :need:`REQ_0814`, :need:`REQ_0815`

.. aou:: Reboot persistence is daemon-side
   :id: AOU_0015
   :status: open

   If post-mortem recovery of FATAL events is required after a
   reboot, the integrator configures the ``dlt-daemon``
   offline-trace storage (``dlt.conf`` ``OfflineTraceDirectory``
   / size limits) — that is the AUTOSAR-spec'd persistence path.
   ``taktora-log-dlt``'s in-memory ring (per :need:`REQ_0814`)
   covers daemon-down windows only and is lost on process restart.

   :Validates: :need:`REQ_0814`
