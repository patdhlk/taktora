Logging — DLT base library with swappable backends
==================================================

Requirements for the workspace-wide logging facade and its default DLT
backend. The chapter introduces two new crates — ``taktora-log`` (the
facade) and ``taktora-log-dlt`` (the DLT backend) — and documents how
they coexist with the existing ``taktora-executor-tracing``.

The design rationale, alternatives considered, and reference deployment
context (COVESA dlt-daemon, AUTOSAR R20-11) live in the companion design
doc ``docs/superpowers/specs/2026-05-18-taktora-log-dlt-design.md``
in the repository root.

The umbrella is split into eight capability-cluster sub-features. Each
sub-feature ``:satisfies:`` an umbrella; each ``req`` ``:satisfies:``
exactly one capability-cluster feature.

Top-level umbrella
------------------

.. feat:: Shared logging base library
   :id: FEAT_0070
   :status: open

   A workspace-wide logging surface used by every taktora crate and by
   any downstream connector. The umbrella satisfies two competing
   forces simultaneously:

   1. **Vehicle integrators want DLT.** The base library must speak
      AUTOSAR Diagnostic Log and Trace natively to a co-located
      COVESA ``dlt-daemon`` so taktora's events surface in the same
      DLT Viewer / dlt-tui / backend-upload pipeline as everything
      else on the ECU.
   2. **Non-vehicle integrators do not want DLT.** Bench rigs, dev
      machines, CI, and third-party experiments must be able to swap
      DLT for ``log4rs`` / ``env_logger`` / a bespoke logger without
      touching any caller site.

   The resolution is to commit to the rust-native ``log`` crate as the
   workspace logging facade (per :need:`CON_0024`) and ship a DLT
   *backend* behind it (per :need:`FEAT_0072`). This mirrors the
   ``embassy-rs/embassy`` posture for std targets — ``log`` is the
   facade, the backend is chosen at process init.

   The umbrella decomposes into the capability clusters below.

----

Capability clusters
-------------------

Facade and backend-swap surface
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

.. feat:: taktora-log facade crate
   :id: FEAT_0071
   :status: open
   :satisfies: FEAT_0070

   A thin facade crate (``taktora-log``) that re-exports the ``log``
   crate's macros, defines the ``LogSink`` extension trait, owns the
   one-shot process init builder, and registers the global ``log::Log``
   implementation exactly once per process. The facade carries no DLT
   knowledge — DLT lives in :need:`FEAT_0072`. Callers depend only on
   ``taktora-log``; the backend is configured at process boot.

.. req:: Single facade for all taktora crates
   :id: REQ_0800
   :status: approved
   :satisfies: FEAT_0071

   Every taktora workspace crate shall emit log records via the ``log``
   crate facade only. No business crate (executor, ``connector-*``,
   replay, bounded-alloc) shall depend directly on ``taktora-log-dlt``
   or any other concrete backend; all calls go through ``log::info!`` /
   ``log::warn!`` / ``log::error!`` / ``log::debug!`` / ``log::trace!``.

.. req:: taktora-log re-exports log macros
   :id: REQ_0801
   :status: approved
   :satisfies: FEAT_0071

   ``taktora-log`` shall re-export the ``log`` crate's macros
   (``info!``, ``warn!``, ``error!``, ``debug!``, ``trace!``, plus
   the structured ``log::kv`` surface) so callers that want a single
   crate dependency can depend on ``taktora-log`` alone and use its
   re-exports as drop-in equivalents of the upstream macros.

.. req:: LogSink trait defines backend extension surface
   :id: REQ_0802
   :status: approved
   :satisfies: FEAT_0071

   ``taktora-log`` shall define a ``LogSink`` trait that captures the
   backend's responsibilities: emit a ``log::Record``, register an
   application name (DLT App ID + Context IDs where applicable),
   accept a runtime log-level change per context, and flush on
   shutdown. Concrete signatures are locked in during implementation;
   the trait surface is the only extension point downstream backends
   target.

.. req:: One-shot init builder selects the backend
   :id: REQ_0803
   :status: approved
   :satisfies: FEAT_0071

   ``taktora-log`` shall expose a builder API that selects the active
   ``LogSink`` implementation at process init and registers it as the
   global ``log::Log`` exactly once. A second init call shall return
   an error rather than silently override. The builder shall accept
   any type implementing ``LogSink``; the default value is
   backend-dependent and resolved by the caller.

.. feat:: Backend-swap surface
   :id: FEAT_0073
   :status: open
   :satisfies: FEAT_0071

   The mechanism by which an integrator replaces the DLT backend with
   ``log4rs``, ``env_logger``, ``tracing-subscriber``, or a bespoke
   logger without touching any caller site. The mechanism is the
   ``log`` crate's own ``set_logger`` plus taktora-log's one-shot
   discipline: an integrator who registers their own logger before
   calling ``taktora-log::init()`` keeps it; ``taktora-log::init()``
   does not override an already-installed logger.

.. req:: Integrator may install any log::Log implementation
   :id: REQ_0804
   :status: approved
   :satisfies: FEAT_0073

   Integrators shall be able to install any ``log::Log`` implementation
   — including ``log4rs``, ``env_logger``, ``tracing-subscriber`` (via
   its ``tracing-log`` consumer side), or a bespoke logger — by
   calling ``log::set_logger`` (directly or through that crate's own
   init helper) **before** invoking ``taktora-log::init()``.
   ``taktora-log::init()`` shall detect the pre-existing logger and
   shall not override it.

.. feat:: tracing-log bridge for existing tracing emitters
   :id: FEAT_0078
   :status: open
   :satisfies: FEAT_0071

   Existing ``tracing::*`` emitters (notably
   ``taktora-executor-tracing``) must keep working without rewrite.
   The bridge captures tracing events as ``log::Record`` so they flow
   through the same active backend as direct ``log::*`` calls. No
   business-code changes are required when a crate moves between
   ``tracing`` and ``log``.

.. req:: tracing-log bridge installed at init
   :id: REQ_0805
   :status: approved
   :satisfies: FEAT_0078

   ``taktora-log::init()`` shall install the ``tracing-log`` bridge
   (``LogTracer::init()`` or equivalent) so events emitted via
   ``tracing::info!`` / ``tracing::warn!`` / ``tracing::error!``
   appear as ``log::Record`` values delivered to the active
   ``LogSink``. The bridge shall be installed exactly once and shall
   not double-emit when a tracing-subscriber is also registered.

----

DLT backend
~~~~~~~~~~~

.. feat:: taktora-log-dlt DLT-protocol backend
   :id: FEAT_0072
   :status: open
   :satisfies: FEAT_0070

   A pure-Rust DLT backend (``taktora-log-dlt``) that implements both
   ``LogSink`` (for use through :need:`FEAT_0071`) and ``log::Log``
   directly (for standalone use without the facade crate). It encodes
   AUTOSAR Classic DLT R20-11 messages via the ``dlt-core`` crate
   (esrlabs), owns a Unix-domain-socket or TCP client to a
   co-located ``dlt-daemon``, and threads the producer hot path
   through a bounded queue and a single background flusher (see
   :need:`ARCH_0072`).

.. req:: AUTOSAR Classic DLT R20-11 encoding via dlt-core
   :id: REQ_0806
   :status: approved
   :satisfies: FEAT_0072

   The DLT backend shall encode every emitted record as an AUTOSAR
   Classic DLT R20-11 message — Storage Header + Standard Header +
   Extended Header + payload — using the ``dlt-core`` crate
   (``esrlabs/dlt-core``) as the encoder. Hand-written byte assembly
   is rejected; schema maintenance lives in the encoder crate.

.. req:: UDS (default) and TCP transports to a local dlt-daemon
   :id: REQ_0807
   :status: approved
   :satisfies: FEAT_0072

   The DLT backend shall support delivery to a co-located
   ``dlt-daemon`` via a Unix-domain socket (default) and via TCP
   (opt-in). The transport shall be selectable through the
   ``taktora-log-dlt`` builder. The backend shall not start, supervise,
   restart, or reconfigure the daemon — that responsibility is the
   integrator's (see :need:`AOU_0010`).

.. req:: 4-character DLT App ID and Context ID per emitting crate
   :id: REQ_0808
   :status: approved
   :satisfies: FEAT_0072

   Each taktora crate that emits log records via ``taktora-log-dlt``
   shall declare a 4-character DLT App ID and one or more 4-character
   Context IDs at init time. taktora reserves the ``TK*`` prefix for
   its own crates (working straw-man: ``TKEX`` taktora-executor,
   ``TKCC`` connector-core, ``TKCH`` connector-host, ``TKZN``
   connector-zenoh, ``TKEC`` connector-ethercat, ``TKCN`` connector-can,
   ``TKCD`` connector-codec, ``TKRP`` replay). Integrators shall pick
   non-``TK*`` IDs (see :need:`AOU_0013`).

.. feat:: Structured key-value fields mapped to DLT verbose args
   :id: FEAT_0074
   :status: open
   :satisfies: FEAT_0072

   The ``log`` crate (v0.4.21+) supports structured key-value pairs
   alongside the formatted message. ``taktora-log-dlt`` shall surface
   those pairs as DLT verbose arguments rather than collapsing them
   into the formatted message, so DLT Viewer / dlt-tui can index on
   them.

.. req:: log::kv pairs encoded as DLT verbose arguments
   :id: REQ_0809
   :status: approved
   :satisfies: FEAT_0074

   For each ``log::Record``, the DLT backend shall iterate
   ``record.key_values()`` and emit one DLT verbose argument per
   structured pair. Native mappings shall apply where the value's
   type matches a DLT primitive: ``u32`` / ``i32`` / ``u64`` / ``i64``
   / ``f32`` / ``f64`` / ``bool`` / ``&str``. Values whose type does
   not have a native DLT mapping shall be rendered via ``Display`` and
   encoded as a verbose string argument. The formatted message body
   shall always be emitted as the leading argument regardless of
   structured-field presence.

Runtime log-level control
~~~~~~~~~~~~~~~~~~~~~~~~~

.. feat:: Runtime per-context log-level control
   :id: FEAT_0075
   :status: open
   :satisfies: FEAT_0072

   DLT clients (``dlt-control``, DLT Viewer's injection panel) can
   change per-Context-ID log levels at runtime by injecting control
   messages into ``dlt-daemon``. ``taktora-log-dlt`` shall consume
   those control messages from the daemon and apply them to its own
   per-context level table without restart. This is the only runtime
   knob taktora-log surfaces to the integrator.

.. req:: Set-Log-Level and Set-Default-Log-Level control messages
   :id: REQ_0810
   :status: approved
   :satisfies: FEAT_0075

   The DLT backend's daemon-client receive half shall ingest
   AUTOSAR DLT Set-Log-Level and Set-Default-Log-Level control
   messages from ``dlt-daemon`` and shall apply them per Context ID
   via atomic store. Subsequent ``log::Log::enabled`` checks for
   that context shall short-circuit at the new level without re-emit
   cost. Unsupported control messages (file transfer, injection,
   FIBEX) shall be ignored without error.

.. req:: Production default level is INFO
   :id: REQ_0811
   :status: approved
   :satisfies: FEAT_0075

   The default log level applied at ``taktora-log-dlt`` init shall be
   ``INFO``. ``DEBUG`` and ``TRACE`` shall be reachable only by
   explicit runtime opt-in through :need:`REQ_0810` (or by an
   integrator's compile-time builder override for development
   builds). The default shall not be ``DEBUG`` in any production
   default build.

Non-blocking hot path and offline buffering
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

.. feat:: Offline ring buffer with reconnect drain
   :id: FEAT_0076
   :status: open
   :satisfies: FEAT_0072

   When the daemon socket is unavailable (early boot, daemon crash,
   daemon restart), the backend shall buffer records in a bounded
   in-memory ring and drain them on reconnect. The hot path stays
   non-blocking regardless of daemon state; overload manifests as
   the documented drop policy, never as a producer-thread stall.

.. req:: Emission shall not block the calling thread
   :id: REQ_0812
   :status: approved
   :satisfies: FEAT_0076

   Emitting a log record via ``log::*`` macros shall not block the
   calling thread. The producer side of ``taktora-log-dlt`` shall be
   a bounded-queue push only; all socket I/O shall happen on the
   backend's own background flusher thread. The producer shall never
   wait on ``connect``, ``write``, or any other socket syscall.

.. req:: ERROR and FATAL emission shall not heap-allocate
   :id: REQ_0813
   :status: approved
   :satisfies: FEAT_0076

   Emitting an ``ERROR``- or ``FATAL``-level record shall not require
   heap allocation on the producer side. Pre-sized record buffers
   shall be used. ``DEBUG`` / ``TRACE`` / ``INFO`` may format-allocate
   under the formatter's usual rules — only ERROR and FATAL are
   covered by the no-alloc guarantee, on the grounds that those are
   the records that must survive even under memory pressure.

.. req:: Bounded in-memory ring buffers records while daemon is down
   :id: REQ_0814
   :status: approved
   :satisfies: FEAT_0076

   When the daemon socket is unavailable, the DLT backend shall
   buffer records in a bounded in-memory ring (capacity configurable
   at init, with a documented safe default). On reconnect the
   backend shall flush the ring in FIFO order before resuming live
   emission. The ring's storage shall be allocated once at init and
   shall not grow at runtime.

.. req:: Drop-oldest overflow policy with summary record on reconnect
   :id: REQ_0815
   :status: approved
   :satisfies: FEAT_0076

   When the in-memory ring is full and a new record arrives, the
   backend shall drop the oldest record, increment an internal drop
   counter, and continue. The backend shall not panic, shall not
   spin, and shall not block. On the next successful reconnect the
   backend shall emit exactly one summary record at the leading
   position of the drain (DLT App ID = the emitter's, Context ID =
   reserved diagnostic context, message body =
   ``taktora.log.dropped count=N first_dropped_at=...``) and reset
   the counter to zero.

Console dev fallback
~~~~~~~~~~~~~~~~~~~~

.. feat:: Console dev fallback
   :id: FEAT_0077
   :status: open
   :satisfies: FEAT_0071

   When no DLT daemon socket is configured and no other ``log::Log``
   implementation has been registered, ``taktora-log::init()`` shall
   install a human-readable console formatter so unit tests and local
   ``cargo run`` produce visible output. Silent drops are explicitly
   rejected.

.. req:: Console fallback installed when no daemon and no other logger
   :id: REQ_0816
   :status: approved
   :satisfies: FEAT_0077

   ``taktora-log::init()`` shall detect the absence of (a) a configured
   DLT daemon socket and (b) any pre-existing ``log::Log``
   implementation, and shall install a console-formatted fallback
   backend in that case. The fallback shall print one line per record
   to ``stderr`` with level, timestamp, target, formatted message,
   and structured key-value pairs. The fallback shall be replaceable
   by any of the other init paths — the fallback is the no-config
   default, not a forced default.
