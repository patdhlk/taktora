# zenoh-pubsub-real

Same shape as `zenoh-pubsub-mock`, but talks to a real `zenoh::Session`
through `RealZenohSession`. Useful for proving the
`taktora-connector-zenoh` `zenoh-integration` feature path actually
works against the upstream `zenoh` crate.

> **CI status**: this example is compile-only in CI. It runs only when
> a developer launches it locally with a peer reachable.

## Run it: two terminals

Terminal A — subscriber:

    cargo run -- --role sub

Terminal B — publisher:

    cargo run -- --role pub --ticks 20

Both processes discover each other as Zenoh peers on loopback using the
default config. Each publish prints `send seq=…`; the subscriber prints
`recv seq=…` for every message that arrives.

For a single-process sanity check (loopback peer):

    cargo run -- --role both --ticks 5

## Troubleshooting peer discovery

- Some hosts firewall the Zenoh peer discovery multicast port.
  If the subscriber never prints, configure explicit TCP locators in
  your local copy by editing `main.rs` — `ZenohConnectorOptions::builder()
  .listen(Locator::new("tcp/127.0.0.1:7447"))` and
  `.connect(Locator::new("tcp/127.0.0.1:7447"))` is the canonical
  loopback pair.
- Confirm both processes report a non-zero peer count after a few
  seconds via the startup health log (`zenoh connector health: ... ->
  Up`).

## What this shows beyond zenoh-pubsub-mock

- Selecting between mock and real Zenoh transport by feature flag
  alone (call sites are otherwise identical).
- `RealZenohSession` ownership of an internal tokio runtime — no
  runtime needs to leak into the executor's WaitSet thread. We spin
  up a small bootstrap runtime only to await `RealZenohSession::open`,
  then drop it; the session's internal zenoh runtime survives.
