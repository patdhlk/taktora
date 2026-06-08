Context, scope, and building blocks
===================================

arc42 §3 (context and scope) and §4 (building blocks) for the logging
stack (:need:`FEAT_0070`). The two architecture directives below frame
the runtime data flow and the control plane; the building blocks then
name the crates and components that realise them.

.. contents:: Sections
   :local:
   :depth: 1

----

Structural overview
-------------------

The logging stack is two crates layered behind one facade: callers emit
through ``taktora-log`` (:need:`BB_0090`); the default backend
``taktora-log-dlt`` (:need:`BB_0091`) encodes and ships DLT to a
co-located daemon via its own daemon client (:need:`BB_0092`).

.. mermaid::

   graph TD
       Caller["any taktora crate<br/>log::* / taktora_log::*"]
       Facade["taktora-log (facade)<br/>BB_0090 — LogSink trait,<br/>init builder, tracing bridge,<br/>console fallback"]
       Backend["taktora-log-dlt (backend)<br/>BB_0091 — dlt-core encoder,<br/>queue + flusher, offline ring,<br/>level table"]
       Client["DLT daemon client<br/>BB_0092 — UDS/TCP, reconnect"]
       Daemon["dlt-daemon<br/>(integrator-provided)"]
       Other["alternative log::Log<br/>(log4rs / env_logger / bespoke)"]

       Caller --> Facade
       Facade -->|"default LogSink"| Backend
       Facade -. "swap path (FEAT_0073)" .-> Other
       Backend --> Client
       Client --> Daemon

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
