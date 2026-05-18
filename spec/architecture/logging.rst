Logging — architecture (arc42)
==============================

Architecture documentation for the workspace-wide logging facade and
its default DLT backend (see :doc:`../requirements/logging`),
structured per the arc42 template and encoded with sphinx-needs using
the useblocks "x-as-code" arc42 directive types. Mirrors the structure
of :doc:`canopen-codegen` for diff-friendly review.

Each architectural element ``:refines:`` or ``:implements:`` a parent
requirement so the trace is preserved end-to-end.

.. contents:: Sections
   :local:
   :depth: 1

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

----

3. Context and scope
--------------------

.. architecture:: Logging runtime data flow
   :id: ARCH_0072
   :status: open
   :refines: FEAT_0070

   Caller code emits via ``log::*`` macros through the global
   ``log::Log`` (set once by ``taktora-log::init()``), which routes
   to the active ``LogSink``. The default ``LogSink`` is
   ``taktora-log-dlt``: it pushes to a bounded queue (no allocation,
   no I/O on the caller's thread), and a single background flusher
   in ``taktora-log-dlt`` encodes via ``dlt-core`` and writes to the
   daemon socket. On daemon-down the flusher diverts to the offline
   ring (per :need:`REQ_0814`) and resumes drain on reconnect.

   .. mermaid::

      graph TB
        Caller["caller code<br/>log::info!(...)"]
        Macro["log crate facade<br/>(global log::Log)"]
        Sink["active LogSink<br/>(default: taktora-log-dlt)"]
        Queue["bounded SPSC/SPMC queue<br/>(no alloc, no I/O)"]
        Flusher["background flusher<br/>(single task)"]
        Encoder["dlt-core encoder<br/>(AUTOSAR R20-11)"]
        Client["daemon client<br/>UDS or TCP"]
        Daemon["dlt-daemon<br/>(integrator-provided)"]
        Ring["offline ring buffer<br/>(bounded, drop-oldest)"]
        Caller --> Macro
        Macro --> Sink
        Sink --> Queue
        Queue --> Flusher
        Flusher --> Encoder
        Encoder --> Client
        Client --> Daemon
        Flusher -. on daemon down .-> Ring
        Ring -. on reconnect .-> Client

.. architecture:: Logging control-plane (runtime level changes)
   :id: ARCH_0073
   :status: open
   :refines: FEAT_0075

   The ``dlt-daemon`` accepts Set-Log-Level / Set-Default-Log-Level
   injections from DLT clients (``dlt-control``, DLT Viewer) and
   forwards them down the same socket connection that
   ``taktora-log-dlt``'s daemon client owns. The receive half decodes
   the control message and updates a per-Context-ID atomic level
   table. Subsequent ``log::Log::enabled`` checks short-circuit
   against that table without re-emit cost.

   .. mermaid::

      graph LR
        Tool["dlt-control /<br/>DLT Viewer"]
        Daemon["dlt-daemon"]
        ClientRx["daemon client<br/>(rx half)"]
        Table["per-context level table<br/>(AtomicU8 per CtxID)"]
        Enabled["log::Log::enabled<br/>short-circuit"]
        Tool -- "Set-Log-Level<br/>(AppID, CtxID, level)" --> Daemon
        Daemon -- "injection" --> ClientRx
        ClientRx --> Table
        Table --> Enabled

----

4. Building blocks
------------------

.. building-block:: taktora-log facade crate
   :id: BB_0090
   :status: open
   :implements: FEAT_0071

   The facade crate. Carries the ``LogSink`` trait surface, the
   one-shot ``init()`` builder, the global ``log::Log`` registration
   helper, the ``tracing-log`` bridge installer, and the console
   dev-fallback formatter. Re-exports the ``log`` crate's macros and
   ``log::kv`` types so a downstream caller can depend on
   ``taktora-log`` alone. Contains no DLT code — DLT lives in
   :need:`BB_0091`. ``std`` only, per :need:`CON_0026`.

.. building-block:: taktora-log-dlt DLT-backend crate
   :id: BB_0091
   :status: open
   :implements: FEAT_0072

   The DLT backend. Implements both ``LogSink`` (for the facade path)
   and ``log::Log`` directly (so the crate is usable without
   :need:`BB_0090` if a caller wants only DLT). Owns the encoder
   (``dlt-core``), the producer→flusher queue, the flusher thread,
   the daemon client, the offline ring, the per-Context-ID level
   table, and the control-message receive path. ``std`` only. No
   ``libdlt`` build dep, per :need:`CON_0025`.

.. building-block:: DLT daemon client (within taktora-log-dlt)
   :id: BB_0092
   :status: open
   :implements: FEAT_0072

   The component inside :need:`BB_0091` that owns the socket
   lifecycle. Single connection, UDS by default, TCP opt-in.
   Reconnect is bounded exponential backoff. The transmit half
   drains the producer queue (or the offline ring during catch-up)
   and writes encoded DLT bytes; the receive half decodes inbound
   control messages and updates the level table. The daemon client
   is module-public so a downstream crate may reuse it without the
   ``log::Log`` integration if they want raw DLT emission only.

----

5. Architecture decisions
-------------------------

.. arch-decision:: Adopt the log crate as workspace logging facade
   :id: ADR_0087
   :status: accepted
   :refines: FEAT_0070

   **Decision.** The workspace logging facade is the ``log`` crate.

   **Forces.** taktora must speak DLT to vehicle integrators and
   plain log macros to non-vehicle integrators. ``log`` is the
   rust-native facade with the widest backend ecosystem (``log4rs``,
   ``env_logger``, ``simplelog``, ``fern``). ``tracing`` is more
   powerful but its swap story (rewriting the Subscriber) is heavier
   for the "drop in log4rs" use case the integrator audience asks
   for.

   **Consequences.** All business crates use ``log::*`` macros.
   Existing ``tracing::*`` callers (``taktora-executor-tracing``)
   continue to work via the ``tracing-log`` bridge (per
   :need:`ADR_0090`). taktora-log re-exports the ``log`` crate's
   macros so callers can depend on the facade crate alone.
   ``embassy-rs/embassy`` follows the same pattern for std targets,
   so the workspace stays idiomatic.

   **Alternatives rejected.** (a) ``tracing`` as primary facade —
   the Subscriber-swap story is heavier for the log4rs use case.
   (b) Both facades coexisting — two ways to do the same thing
   doubles the spec surface. (c) A custom ``taktora-log`` trait
   surface — every downstream backend would need to learn a
   non-standard facade.

.. arch-decision:: Pure-Rust DLT via dlt-core; no libdlt FFI
   :id: ADR_0088
   :status: accepted
   :refines: FEAT_0072

   **Decision.** The DLT backend encodes via ``esrlabs/dlt-core``
   (pure Rust, AUTOSAR R20-11 conformant). No ``libdlt`` /
   ``dlt-sys`` / ``dlt-rs`` / ``dlt_log`` in the default Cargo graph.

   **Forces.** Vehicle integrators want bit-compatible DLT. Other
   integrators want a clean Rust crate that cross-compiles without
   ``bindgen`` against a C library that is not always present.

   **Consequences.** Every consumer compiles taktora's DLT backend
   without installing ``libdlt-dev``. Cross-compilation to
   non-Linux targets (e.g. macOS dev hosts that talk to a remote
   ``dlt-daemon`` over TCP) works out of the box. The daemon client
   is taktora's responsibility — no existing pure-Rust crate
   combines ``dlt-core``'s encoder with a UDS/TCP daemon client.

   **Alternatives rejected.** (a) FFI to ``libdlt`` — adds a system
   dep on every consumer, complicates cross-compile, blocks
   non-Linux dev hosts. (b) Reuse ``rusty-projects/dlt_log`` —
   smallest implementation effort but it is FFI-based, has no
   structured-field support, and forces ``libdlt`` on every
   consumer. (c) Reuse Eclipse OpenSOVD's ``tracing-dlt`` — ties to
   ``tracing`` facade and FFI to ``libdlt`` — rejected on both
   axes.

.. arch-decision:: Two-crate split (facade vs DLT backend)
   :id: ADR_0089
   :status: accepted
   :refines: FEAT_0070

   **Decision.** Split the workspace surface into two crates —
   ``taktora-log`` (facade + ``LogSink`` trait + init + dev
   fallback + tracing bridge) and ``taktora-log-dlt`` (DLT
   backend). The DLT backend is consumable on its own without the
   facade crate.

   **Forces.** A consumer that wants only DLT (e.g. an integrator
   wiring DLT into their own non-``log`` framework) should not
   have to pay for the facade crate. A consumer that wants only
   the facade (e.g. with ``log4rs``) should not have to compile
   ``dlt-core`` and the daemon client. The two audiences are
   asymmetric and the split is cheap.

   **Consequences.** Two ``Cargo.toml`` files instead of one. Both
   crates ship with their own version cadence under
   ``release-plz``. The DLT crate is module-public so its daemon
   client (:need:`BB_0092`) is reusable standalone.

   **Alternatives rejected.** (a) One crate with a ``dlt`` cargo
   feature — feature-gating the DLT backend in the same crate as
   the facade muddies the trait surface and bloats the dependency
   graph for users with ``default-features = false``. (b) Three
   crates (facade + DLT + console) — over-decomposition; the
   console fallback is small enough to live in the facade crate.

.. arch-decision:: Bridge existing tracing emitters via tracing-log
   :id: ADR_0090
   :status: accepted
   :refines: FEAT_0078

   **Decision.** Existing ``tracing::*`` emitters
   (``taktora-executor-tracing`` and any future tracing-using
   crate) are captured via the ``tracing-log`` bridge rather than
   being rewritten to use ``log::*`` directly.

   **Forces.** ``taktora-executor-tracing`` is the executor's
   ``Observer`` impl and is referenced in the executor spec.
   Rewriting it touches a lot of existing tests and risks
   regressions for no architectural gain. The ``tracing-log``
   bridge is one ``LogTracer::init()`` call and captures every
   ``tracing::Event`` as a ``log::Record``.

   **Consequences.** Only one output pipeline, regardless of
   whether the emitter chose ``log`` or ``tracing``. No business-
   code changes required when a crate moves between the two
   macro sets. The bridge must be installed before any tracing
   subscriber is registered — otherwise the events go to the
   subscriber and bypass the bridge.

   **Alternatives rejected.** (a) Rewrite ``taktora-executor-tracing``
   to use ``log::*`` — gratuitous churn. (b) Ship two parallel
   pipelines — duplicates plumbing and confuses consumers.

.. arch-decision:: Console dev fallback when no daemon configured
   :id: ADR_0091
   :status: accepted
   :refines: FEAT_0077

   **Decision.** When ``taktora-log::init()`` finds no DLT daemon
   socket configured and no pre-existing ``log::Log``, it installs
   a human-readable console formatter that writes one line per
   record to ``stderr``.

   **Forces.** First-time local ``cargo run`` and unit tests
   should produce visible output without the integrator standing
   up a ``dlt-daemon`` first. Silent drops on no-config are a
   debugging trap.

   **Consequences.** Unit tests and local development "just work".
   The fallback is replaced by any of the other init paths —
   explicit daemon config, pre-installed ``log4rs``, or an
   integrator-supplied ``LogSink``. The fallback formatter is small
   enough to live in ``taktora-log`` without pulling extra deps.

   **Alternatives rejected.** (a) No fallback (silent drops) — a
   debugging trap; rejected. (b) Panic on no-config — surprises
   first-time users; rejected. (c) Write to a file — requires
   choosing a path; rejected as more configuration than a fallback
   should require.
