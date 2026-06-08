Process boundary deployments
============================

The two deployment shapes the framework supports, sharing one envelope
contract. This cluster ``:satisfies:`` :need:`FEAT_0030`.

.. feat:: Process boundary deployments
   :id: FEAT_0035
   :status: open
   :satisfies: FEAT_0030

   The framework supports two deployment shapes — gateway as an in-process
   tokio task or as a separate gateway binary — using the same envelope
   contract on both sides.

.. req:: Same envelope contract for both deployments
   :id: REQ_0240
   :status: approved
   :satisfies: FEAT_0035

   The framework shall use the same ``ConnectorEnvelope`` definition,
   iceoryx2 service shape, and ``ChannelDescriptor`` semantics regardless
   of whether the gateway runs in-process or as a separate binary.

.. req:: In-process gateway is a tokio task
   :id: REQ_0241
   :status: open
   :satisfies: FEAT_0035

   The framework shall support running the gateway as a tokio task spawned
   by ``ConnectorHost`` alongside the plugin's executor, in a single
   process.

.. req:: Separate-process gateway is a self-contained binary
   :id: REQ_0242
   :status: open
   :satisfies: FEAT_0035

   The framework shall support running the gateway as a self-contained
   binary in its own OS process, communicating with the plugin only
   through iceoryx2 shared memory.

.. req:: Clean exit on SIGINT / SIGTERM on both sides
   :id: REQ_0243
   :status: open
   :satisfies: FEAT_0035

   Both the plugin host and a separate gateway binary shall return cleanly
   from ``Executor::run()`` on SIGINT/SIGTERM, drain any tokio runtime
   sidecar, and release iceoryx2 services.

.. req:: No app↔gateway control-plane envelopes
   :id: REQ_0244
   :status: approved
   :satisfies: FEAT_0035

   The framework shall not introduce envelopes carrying control-plane
   semantics ("ping", "version", "shutdown handshake") on the SHM channel.
   Health is observed via ``ConnectorHealth``, not negotiated.
