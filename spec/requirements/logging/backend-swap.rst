Backend-swap surface
====================

How an integrator replaces the DLT backend with ``log4rs`` /
``env_logger`` / ``tracing-subscriber`` / a bespoke logger without
touching any caller site, riding on the ``log`` crate's own
``set_logger`` plus the facade's one-shot init discipline
(:need:`FEAT_0071`).

.. feat:: Backend-swap surface
   :id: FEAT_0073
   :status: implemented
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
   :status: implemented
   :satisfies: FEAT_0073
   :links: BB_0090, TEST_0804

   Integrators shall be able to install any ``log::Log`` implementation
   — including ``log4rs``, ``env_logger``, ``tracing-subscriber`` (via
   its ``tracing-log`` consumer side), or a bespoke logger — by
   calling ``log::set_logger`` (directly or through that crate's own
   init helper) **before** invoking ``taktora-log::init()``.
   ``taktora-log::init()`` shall detect the pre-existing logger and
   shall not override it.
