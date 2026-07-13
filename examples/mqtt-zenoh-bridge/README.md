# mqtt-zenoh-bridge — the golden path

A protocol bridge in one file: **MQTT ingress → an executor transform →
Zenoh egress**. A synthetic sensor publishes temperature readings onto an
MQTT topic; one executor item drains them, does real work (Celsius →
Fahrenheit plus a threshold alarm decision), and re-publishes the decision
onto a Zenoh key expression, where a sink item reads it back.

This is the **start-here example** because it shows the whole connector
framework's value proposition at once, with nothing to install:

- One `Executor` drives **two unrelated wire protocols** (MQTT and Zenoh).
- Both are driven through the **same `ChannelReader` / `ChannelWriter`
  seam** — the bridge item never mentions MQTT or Zenoh.
- No broker, no router, no hardware: both connectors run against in-process
  **mock sessions**, so `cargo run` just works.

## Run it

    cd examples/mqtt-zenoh-bridge
    cargo run -- --ticks 5

`--ticks N` injects N readings, bridges each one, then exits with a summary:

    mqtt  in : seq=0 celsius=40.0
    zenoh out: seq=0 celsius=40.0 fahrenheit=104.0 level=Ok
    ...
    mqtt  in : seq=3 celsius=85.0
    zenoh out: seq=3 celsius=85.0 fahrenheit=185.0 level=Critical
    ingested=5 bridged=5 egress=5

The Celsius value ramps across ticks so the alarm decision visibly changes
(`Ok` → `Warn` → `Critical`) — proof the executor item is doing real work,
not forwarding bytes.

## The stack

Seven published `taktora-*` crates (plus `taktora-stats`, pulled in
transitively) compose this example. Each has one job:

| Crate | Role in this example |
|---|---|
| `taktora-executor` | The cyclic executor. Schedules interval-triggered work items (`item_with_triggers`) across worker threads; owns the run loop and `ControlFlow` / `stop_executor`. |
| `taktora-connector-core` | The **protocol-agnostic seam**. Defines `ChannelDescriptor`, the typed `ChannelReader` / `ChannelWriter` returned by every connector, and the `PayloadCodec` trait. This is the abstraction that lets one transform span two protocols. |
| `taktora-connector-host` | The `Connector` trait: `create_reader` / `create_writer` / `register_with` / `subscribe_health` / `health`. The host-side glue that binds a connector to the executor. |
| `taktora-connector-codec` | Payload codecs. Here, `JsonCodec` serializes the typed `SensorReading` / `AlarmDecision` structs to and from wire bytes on both connectors. |
| `taktora-connector-transport-iox` | The iceoryx2 shared-memory transport under the reader/writer channels — the raw pub/sub carrying bytes between a connector's gateway and the work items. Pulled in transitively; you never call it directly. |
| `taktora-connector-mqtt` | The **ingress** connector: `MqttConnector`, `MqttState`, `MqttRouting` / `MqttTopic` / `MqttQos`, and `MockMqttSession` (loops delivered bytes to matching subscriptions in-process). |
| `taktora-connector-zenoh` | The **egress** connector: `ZenohConnector`, `ZenohState`, `ZenohRouting` / `KeyExprOwned`, and `MockZenohSession` (dispatches publishes to matching subscribers in-process). |

## Walkthrough, in code order

`src/main.rs` is wired top to bottom:

1. **Ingress connector (MQTT).** Build an `MqttConnector` over a
   `MockMqttSession`, then create a **reader** on the sensor topic. The
   reader is the ingress seam; created before `register_with` so its
   subscription is live before the first delivery.

2. **Egress connector (Zenoh).** Build a `ZenohConnector` over a
   `MockZenohSession`, then create a **writer** on the alarms key (the
   egress seam) and a matching **reader** so the sink can observe what was
   published (the mock loops it back).

3. **One executor, both connectors.** `Executor::builder().build()`, then
   `mqtt.register_with(&mut exec)?` **and** `zenoh.register_with(&mut
   exec)?`. This is the load-bearing fact: two independent connectors, each
   spawning its own gateway dispatcher, share a single executor.

4. **Health subscriptions** for both connectors, taken before work items
   are added so the run observes the `Connecting → Up` transitions.

5. **Ingress simulator item.** Stands in for a real MQTT device: every
   200 ms it encodes a `SensorReading` and hands it to
   `session.deliver_inbound(...)`, exactly as a broker would deliver a
   publish. After N readings it returns `StopChain` to retire itself — it
   does **not** stop the executor, so the pipeline can finish draining.

6. **The bridge item — the whole point.** Every 50 ms it drains the ingress
   reader, runs `transform(...)`, and sends the result on the egress writer:

   ```rust
   while let Ok(Some(env)) = ingress.try_recv() {   // MQTT reader
       let decision = transform(&env.value);         // real work
       egress.send(&decision)?;                       // Zenoh writer
   }
   ```

   Nothing here names MQTT or Zenoh. The same two calls — `try_recv()` and
   `send(...)` — span two protocols. Swap either mock for its real session,
   or Zenoh for EtherCAT, and this code does not change.

7. **Egress sink item.** Drains the Zenoh reader, prints each decision, and
   once all N readings have completed the two-hop journey it calls
   `ctx.stop_executor()` — the natural end of the run.

8. **Health polling item** logs any state transitions on either connector.

9. **`exec.run()`**, then a `ingested=N bridged=N egress=N` summary; a
   non-zero exit if the bridge produced decisions that never reached the
   sink.

## What to tweak

- **The transform** (`fn transform`): change the thresholds (`WARN_C`,
  `CRITICAL_C`) or the conversion. This is the "business logic" the bridge
  exists to run.
- **The wire topics**: `MQTT_TOPIC` and `ZENOH_KEY`.
- **`--ticks N`**: how many readings to bridge before exiting.
- **Go real**: replace `MockMqttSession` / `MockZenohSession` with the real
  sessions (see the `mqtt-pubsub-real` and `zenoh-pubsub-real` examples).
  The bridge item is untouched.

## Debugging against in-tree crates

By default this example builds against the published crates.io versions.
To point it at the local `../../crates/taktora-*` sources, use the repo-root
toggle:

    scripts/examples-local.sh on
    cd examples/mqtt-zenoh-bridge && cargo run
    scripts/examples-local.sh off        # restore before committing

Never commit with the toggle left `on`.
