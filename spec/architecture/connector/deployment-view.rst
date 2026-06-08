Deployment view
===============

arc42 §7.

The framework supports two deployment shapes from the same envelope
contract. Operators choose based on fault-isolation requirements; the
plugin's code is unchanged across both.

.. architecture:: In-process gateway deployment
   :id: ARCH_0020
   :status: open
   :refines: REQ_0240, REQ_0241

   One OS process; the host launches both the plugin's executor and a
   tokio task hosting ``MqttGateway``. SHM transport is in-process
   shared memory between two threads of the same process.

   .. mermaid::

      flowchart LR
        subgraph one_process[Single OS process]
          direction LR
          PLUGIN[Plugin executor<br/>taktora-executor]
          GATEWAY[Gateway tokio task<br/>rumqttc + bridge]
          SHM[(iceoryx2 SHM)]
          PLUGIN <--> SHM <--> GATEWAY
        end
        BROKER[(MQTT broker)]
        GATEWAY <--> BROKER

   **Pros.** Simpler ops (one binary, one signal handler, one log
   stream). No SHM pool sizing for a peer process. **Cons.** A panic
   in the tokio task aborts the application — loses :need:`QG_0001`.
   Recommended for development, examples, and small deployments where
   protocol-stack stability is trusted.

.. architecture:: Separate-process gateway deployment
   :id: ARCH_0021
   :status: open
   :refines: REQ_0240, REQ_0242

   Two OS processes; each runs its own taktora-executor. The plugin's
   process embeds only ``ConnectorHost``; the gateway's process
   embeds ``ConnectorGateway`` + the protocol stack. SHM transport is
   inter-process shared memory.

   .. mermaid::

      flowchart LR
        subgraph plugin_proc[Plugin process]
          PLUGIN[Plugin executor<br/>taktora-executor]
        end
        subgraph shm[iceoryx2 SHM]
          POOL[(shared memory pool)]
        end
        subgraph gw_proc[Gateway process]
          GATEWAY[Gateway executor + tokio<br/>rumqttc + bridge]
        end
        PLUGIN <--> POOL <--> GATEWAY
        BROKER[(MQTT broker)]
        GATEWAY <--> BROKER

   **Pros.** Full fault isolation — a panic in the gateway crashes the
   gateway only; the plugin observes ``HealthEvent::Down`` and the app
   stays alive. Independent restart policies. **Cons.** Two binaries
   to deploy and supervise; SHM pool sizing must be planned for the
   peer process; clean shutdown requires SIGINT to both halves.
   Recommended for production deployments where :need:`QG_0001`
   matters.
