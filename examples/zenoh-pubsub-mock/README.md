# zenoh-pubsub-mock

Executor + `taktora-connector-zenoh` + `JsonCodec` end-to-end, backed by
`MockZenohSession`. No router, no network, no `zenoh-integration`
feature flag — everything happens in-process.

## Run it

    cargo run

Or bound the duration:

    cargo run -- --ticks 20

Each tick prints two lines: `send seq=N ts_ms=…` then (once the
dispatcher has had time to round-trip the bytes through the mock
session) `recv seq=N ts_ms=…`. The example exits after `--ticks N`
publishes (default `10`) or on Ctrl-C.

## What this shows

- `ZenohConnector::new(state, session, JsonCodec)` with the mock session.
- Creating a `ChannelDescriptor<ZenohRouting, 256>` and a paired
  reader + writer (reader first, before any publish reaches the
  dispatcher).
- `Connector::register_with(&mut executor)` to install the gateway-side
  dispatcher into the executor.
- Two `ExecutableItem`s composed via `item_with_triggers`: one on a
  200ms interval that publishes, one on a 50ms interval that drains.

## What to tweak

- `--ticks`, the `Duration::from_millis(200)` publish interval, or the
  `dispatcher_tick` on `ZenohConnectorOptions::builder()`.
- The `Tick { seq, ts_ms }` payload — any `serde::Serialize +
  DeserializeOwned` type works.
- Swap `MockZenohSession` for `RealZenohSession` (and add the
  `zenoh-integration` feature on `taktora-connector-zenoh`) — see the
  sibling `zenoh-pubsub-real` example.
