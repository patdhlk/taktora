Logging — architecture (arc42)
==============================

Architecture documentation for the workspace-wide logging facade and
its default DLT backend (see :doc:`../../requirements/logging/index`),
structured per the arc42 template and encoded with sphinx-needs using
the useblocks "x-as-code" arc42 directive types. Mirrors the structure
of :doc:`../canopen-codegen` for diff-friendly review.

Each architectural element ``:refines:`` or ``:implements:`` a parent
requirement so the trace is preserved end-to-end.

This chapter is split across pages (see the toctree): the framing
sections §1–§2 (goals and constraints) live here on the index; the
context-and-scope flows and the building-block decomposition (§3–§4)
live in :doc:`building-blocks`; the runtime data-flow and control-plane
sequences live in :doc:`runtime-view`; and the architecture decisions
(§5) live in :doc:`decisions`.

----

1. Introduction and goals
-------------------------

The chapter's reason-to-exist is **a single logging surface that all
taktora crates emit through, with DLT as the default backend and a
clear swap path for ``log4rs`` / ``env_logger`` / bespoke loggers**.
Vehicle integrators get AUTOSAR-spec'd DLT to a co-located
``dlt-daemon``; non-vehicle integrators replace the backend at
process boot without touching any caller site.

Quality goals capture the qualities the architecture is optimised for.

.. quality-goal:: Backend decoupling (single facade, replaceable backend)
   :id: QG_0018
   :status: open
   :refines: FEAT_0070

   Every taktora crate shall emit through one stable facade — the
   ``log`` crate — so the concrete backend can be replaced without
   touching any caller. The facade is non-negotiable; the backend is
   a deployment choice. This is what lets a CI run with
   ``env_logger`` use the same business code that a vehicle ECU
   runs with DLT.

.. quality-goal:: DLT-ecosystem observability
   :id: QG_0019
   :status: open
   :refines: FEAT_0070

   taktora's events shall surface in the standard COVESA DLT
   ecosystem — DLT Viewer, dlt-tui, ``dlt-daemon`` gateway-mode
   aggregation, backend upload — without requiring custom adapters
   on the consumer side. Wire-level compatibility with AUTOSAR
   Classic DLT R20-11 is the contract.

.. quality-goal:: Low-overhead, non-blocking hot path
   :id: QG_0020
   :status: open
   :refines: FEAT_0070

   Emitting a log record shall not block the calling thread, shall
   not allocate on the producer side for ERROR / FATAL records, and
   shall not require coordinated state between producer and flusher
   beyond a bounded SPSC/SPMC queue. Overload manifests as the
   documented drop policy, never as a stall in the executor.

.. quality-goal:: Dev-friendly fallback (no daemon required)
   :id: QG_0021
   :status: open
   :refines: FEAT_0070

   Local development and CI must work without a running
   ``dlt-daemon``. The console-formatted fallback (per
   :need:`FEAT_0077`) is the default behaviour when no daemon socket
   is configured, so newcomers see log output the first time they
   ``cargo run``.

----

2. Constraints
--------------

.. constraint:: log crate as workspace logging facade
   :id: CON_0024
   :status: open
   :refines: FEAT_0070

   The workspace logging facade shall be the ``log`` crate
   (``rust-lang/log``, v0.4.21 or newer for ``kv`` support). The
   ``tracing`` crate remains in the workspace for its span model
   in :need:`BB_0090` and is bridged into ``log`` via the
   ``tracing-log`` consumer (per :need:`REQ_0805`). No third facade
   is introduced.

.. constraint:: No build-time dependency on libdlt
   :id: CON_0025
   :status: open
   :refines: FEAT_0072

   ``taktora-log-dlt`` shall not depend on the C ``libdlt`` library
   at build time. No ``libdlt-sys`` / ``dlt-sys`` / ``dlt-rs`` /
   ``dlt_log`` (rusty-projects) in the default Cargo graph. The
   DLT codec is pure Rust via ``esrlabs/dlt-core`` (per
   :need:`ADR_0088`). Integrators with bit-for-bit ``libdlt``
   parity needs bring their own ``LogSink`` impl behind
   :need:`FEAT_0073`.

.. constraint:: std required, no_std out of scope
   :id: CON_0026
   :status: open
   :refines: FEAT_0070

   Both ``taktora-log`` and ``taktora-log-dlt`` shall require ``std``.
   ``no_std`` support is out of scope for this round — taktora's
   targets are all std platforms. A future MCU connector needing DLT
   will get its own spec covering ``defmt``-style emission with a
   host-side adapter.

.. constraint:: Logging is QM
   :id: CON_0027
   :status: open
   :refines: FEAT_0070

   Logging is treated as QM. No ``tsr`` is committed against
   ``taktora-log`` or ``taktora-log-dlt``. The freedom-from-
   interference posture is carried as Assumptions of Use on the
   integrator side (see :need:`AOU_0010` through :need:`AOU_0015`).
   Integrators who need a certified path bring their own backend
   behind :need:`FEAT_0073`; the safety case is then theirs.

.. toctree::
   :maxdepth: 2

   building-blocks
   runtime-view
   decisions
