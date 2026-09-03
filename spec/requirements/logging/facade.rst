Facade — taktora-log crate
==========================

The thin facade crate that all taktora crates emit through. It owns the
``LogSink`` extension trait, the one-shot init builder, and the global
``log::Log`` registration — but carries no DLT knowledge (see
:need:`FEAT_0070`).

.. feat:: taktora-log facade crate
   :id: FEAT_0071
   :status: implemented
   :satisfies: FEAT_0070

   A thin facade crate (``taktora-log``) that re-exports the ``log``
   crate's macros, defines the ``LogSink`` extension trait, owns the
   one-shot process init builder, and registers the global ``log::Log``
   implementation exactly once per process. The facade carries no DLT
   knowledge — DLT lives in :need:`FEAT_0072`. Callers depend only on
   ``taktora-log``; the backend is configured at process boot.

.. req:: Single facade for all taktora crates
   :id: REQ_0800
   :status: implemented
   :satisfies: FEAT_0071
   :links: BB_0090, TEST_0800

   Every taktora workspace crate shall emit log records via the ``log``
   crate facade only. No business crate (executor, ``connector-*``,
   replay, bounded-alloc) shall depend directly on ``taktora-log-dlt``
   or any other concrete backend; all calls go through ``log::info!`` /
   ``log::warn!`` / ``log::error!`` / ``log::debug!`` / ``log::trace!``.

.. req:: taktora-log re-exports log macros
   :id: REQ_0801
   :status: implemented
   :satisfies: FEAT_0071
   :links: BB_0090, TEST_0801

   ``taktora-log`` shall re-export the ``log`` crate's macros
   (``info!``, ``warn!``, ``error!``, ``debug!``, ``trace!``, plus
   the structured ``log::kv`` surface) so callers that want a single
   crate dependency can depend on ``taktora-log`` alone and use its
   re-exports as drop-in equivalents of the upstream macros.

.. req:: LogSink trait defines backend extension surface
   :id: REQ_0802
   :status: implemented
   :satisfies: FEAT_0071
   :links: BB_0090, TEST_0802

   ``taktora-log`` shall define a ``LogSink`` trait that captures the
   backend's responsibilities: emit a ``log::Record``, register an
   application name (DLT App ID + Context IDs where applicable),
   accept a runtime log-level change per context, and flush on
   shutdown. Concrete signatures are locked in during implementation;
   the trait surface is the only extension point downstream backends
   target.

.. req:: One-shot init builder selects the backend
   :id: REQ_0803
   :status: implemented
   :satisfies: FEAT_0071
   :links: BB_0090, TEST_0803

   ``taktora-log`` shall expose a builder API that selects the active
   ``LogSink`` implementation at process init and registers it as the
   global ``log::Log`` exactly once. A second init call shall return
   an error rather than silently override. The builder shall accept
   any type implementing ``LogSink``; the default value is
   backend-dependent and resolved by the caller.
