Host wiring and builder
=======================

The composition layer that wraps a ``taktora_executor::Executor``. This
cluster ``:satisfies:`` :need:`FEAT_0030`.

.. feat:: Host wiring and builder
   :id: FEAT_0037
   :status: open
   :satisfies: FEAT_0030

   The composition layer that wraps a ``taktora_executor::Executor`` and
   registers each connector's contributed ``ExecutableItem`` instances —
   matching taktora-executor's existing builder idiom.

.. req:: ConnectorHost builder API
   :id: REQ_0270
   :status: approved
   :satisfies: FEAT_0037

   ``taktora-connector-host`` shall expose
   ``ConnectorHost::builder()...with(connector)...build()`` returning a
   ``ConnectorHost`` that owns a ``taktora_executor::Executor``.

.. req:: ConnectorGateway builder API
   :id: REQ_0271
   :status: approved
   :satisfies: FEAT_0037

   ``taktora-connector-host`` shall expose a parallel
   ``ConnectorGateway::builder()`` for the gateway-side composition,
   producing a binary that owns its own ``taktora_executor::Executor``.

.. req:: Host registers connector items with the executor
   :id: REQ_0272
   :status: approved
   :satisfies: FEAT_0037

   ``ConnectorHost::build()`` shall call ``Executor::add`` for every
   ``ExecutableItem`` contributed by registered connectors and shall
   return an executor ready to run.

.. req:: Optional Observer adapter for tracing
   :id: REQ_0273
   :status: open
   :satisfies: FEAT_0037

   Behind a default-off ``tracing`` cargo feature, the host shall provide
   an ``Observer`` adapter (using ``taktora-executor-tracing``) that
   forwards ``HealthEvent`` and ``ExecutionMonitor`` callbacks through
   the global ``tracing`` subscriber.
