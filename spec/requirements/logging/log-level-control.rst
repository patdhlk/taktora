Runtime log-level control
=========================

The only runtime knob taktora-log surfaces: DLT clients change
per-Context-ID log levels at runtime by injecting control messages into
``dlt-daemon``, which the DLT backend (:need:`FEAT_0072`) consumes and
applies without restart.

.. feat:: Runtime per-context log-level control
   :id: FEAT_0075
   :status: approved
   :satisfies: FEAT_0072

   DLT clients (``dlt-control``, DLT Viewer's injection panel) can
   change per-Context-ID log levels at runtime by injecting control
   messages into ``dlt-daemon``. ``taktora-log-dlt`` shall consume
   those control messages from the daemon and apply them to its own
   per-context level table without restart. This is the only runtime
   knob taktora-log surfaces to the integrator.

.. req:: Set-Log-Level and Set-Default-Log-Level control messages
   :id: REQ_0810
   :status: implemented
   :satisfies: FEAT_0075
   :links: BB_0091, TEST_0810

   The DLT backend's daemon-client receive half shall ingest
   AUTOSAR DLT Set-Log-Level and Set-Default-Log-Level control
   messages from ``dlt-daemon`` and shall apply them per Context ID
   via atomic store. Subsequent ``log::Log::enabled`` checks for
   that context shall short-circuit at the new level without re-emit
   cost. Unsupported control messages (file transfer, injection,
   FIBEX) shall be ignored without error.

.. req:: Production default level is INFO
   :id: REQ_0811
   :status: implemented
   :satisfies: FEAT_0075
   :links: BB_0091, TEST_0811

   The default log level applied at ``taktora-log-dlt`` init shall be
   ``INFO``. ``DEBUG`` and ``TRACE`` shall be reachable only by
   explicit runtime opt-in through :need:`REQ_0810` (or by an
   integrator's compile-time builder override for development
   builds). The default shall not be ``DEBUG`` in any production
   default build.
