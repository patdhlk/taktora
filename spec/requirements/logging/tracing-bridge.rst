tracing-log bridge
==================

Keeps existing ``tracing::*`` emitters (notably
``taktora-executor-tracing``) working without rewrite by capturing
their events as ``log::Record`` through the same active backend
(:need:`FEAT_0071`).

.. feat:: tracing-log bridge for existing tracing emitters
   :id: FEAT_0078
   :status: implemented
   :satisfies: FEAT_0071

   Existing ``tracing::*`` emitters (notably
   ``taktora-executor-tracing``) must keep working without rewrite.
   The bridge captures tracing events as ``log::Record`` so they flow
   through the same active backend as direct ``log::*`` calls. No
   business-code changes are required when a crate moves between
   ``tracing`` and ``log``.

.. req:: tracing-log bridge installed at init
   :id: REQ_0805
   :status: implemented
   :satisfies: FEAT_0078
   :links: BB_0090, TEST_0805

   ``taktora-log::init()`` shall install the ``tracing-log`` bridge
   (``LogTracer::init()`` or equivalent) so events emitted via
   ``tracing::info!`` / ``tracing::warn!`` / ``tracing::error!``
   appear as ``log::Record`` values delivered to the active
   ``LogSink``. The bridge shall be installed exactly once and shall
   not double-emit when a tracing-subscriber is also registered.
