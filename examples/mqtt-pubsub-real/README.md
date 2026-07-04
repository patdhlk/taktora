# mqtt-pubsub-real

Executor + `taktora-connector-mqtt` + `JsonCodec` end-to-end, backed by
`RealMqttSession` — the `rumqttc-integration` backend talking to an
**actual** MQTT broker. Unlike [`mqtt-pubsub-mock`](../mqtt-pubsub-mock),
the loopback is genuine: the app publishes to a topic it is also
subscribed to, and the broker echoes each message back. No
`deliver_inbound` seam — the round-trip goes out over TCP and comes back.

## Run it in a container (recommended)

The bundled compose file stands up a mosquitto broker and runs the app
against it:

    docker compose up --build --abort-on-container-exit

You'll see the app connect and print interleaved `send seq=N …` /
`recv seq=N …` lines, then exit 0 after 20 ticks. Tear down with:

    docker compose down

## Run it against your own broker

Point the binary at any broker with `--broker`/`--port` (or the
`MQTT_BROKER` / `MQTT_PORT` env vars):

    cargo run --release -- --broker 127.0.0.1 --port 1883 --ticks 20

Without a reachable broker the connector stays in `Connecting` and no
`recv` lines appear — that's expected; this example needs a real broker,
which is why CI only **builds** it (the container is the run path).

## What this shows

- `MqttConnector::new(state, Arc<RealMqttSession>, JsonCodec)` over the
  real rumqttc stack.
- The runtime nuance: `RealMqttSession::connect` spawns an event-loop
  *pump* on the tokio runtime it is called from, while the sync
  `Executor::run()` blocks the main thread. So `main` builds a
  multi-thread tokio runtime, `block_on`s the connect, and **keeps the
  runtime in scope** for the whole run so its workers keep the pump
  alive. (Contrast `zenoh-pubsub-real`, whose session owns an internal
  runtime and can drop the bootstrap one.)
- A `ChannelDescriptor<MqttRouting, 256>` (topic
  `taktora/examples/pubsub-real`, QoS 1) with a paired reader + writer
  (reader first, so the broker SUBSCRIBE lands before the first publish).
- `Connector::register_with(&mut executor)` to install the gateway-side
  dispatcher, plus three `item_with_triggers` items: a 200ms publisher,
  a 50ms drain, and a 250ms health-transition logger.

## The container files

- `mosquitto.conf` — anonymous plain-TCP listener on 1883.
- `Dockerfile` — multi-stage Rust build (against the published crates.io
  deps, patch off) → slim runtime image running the binary.
- `docker-compose.yml` — `broker` (`eclipse-mosquitto:2`) + `app` (built
  from the Dockerfile, `MQTT_BROKER=broker`).
