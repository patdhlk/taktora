# mqtt-pubsub-mock

Executor + `taktora-connector-mqtt` + `JsonCodec` end-to-end, backed by
`MockMqttSession`. No broker, no network, no `rumqttc-integration`
feature flag — everything happens in-process.

## Run it

    cargo run

Or bound the duration:

    cargo run -- --ticks 20

Each tick prints two lines: `send seq=N ts_ms=…` then (once the
dispatcher has had time to loop the bytes back through the mock session)
`recv seq=N ts_ms=…`. The example exits after `--ticks N` publishes
(default `10`) or on Ctrl-C.

## What this shows

- `MqttConnector::new(state, session, JsonCodec)` with `MockMqttSession`.
- A `ChannelDescriptor<MqttRouting, 256>` (topic `taktora/examples/pubsub`,
  QoS 1) and a paired reader + writer (reader first, so the subscription
  is in place before any publish reaches the dispatcher).
- `Connector::register_with(&mut executor)` to install the gateway-side
  dispatcher into the executor.
- Three `ExecutableItem`s composed via `item_with_triggers`: a 200ms
  publisher, a 50ms drain, and a 250ms health-transition logger.

## Real broker

To talk to an actual MQTT broker, use the `RealMqttSession` behind the
connector's `rumqttc-integration` feature instead of `MockMqttSession`
(see `taktora-connector-mqtt`'s `real` module and the broker-in-CI
integration tests). This mock example keeps the focus on the wiring.
