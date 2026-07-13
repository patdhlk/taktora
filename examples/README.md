# Integration examples

Each subdirectory here is a **standalone Cargo crate** that depends on
the published `taktora-* = "0.1"` crates from crates.io. They are not
workspace members, so each carries its own `Cargo.lock`. The intent
is that every example reads like a manifest a downstream user might
write, with no workspace-relative paths active by default.

## What's here

| Example | Backend | CI | What it shows |
|---|---|---|---|
| [`mqtt-zenoh-bridge`](mqtt-zenoh-bridge)   | Mock MQTT+Zenoh      | build + run | **Start here.** One executor bridging two protocols across the same `ChannelReader` / `ChannelWriter` seam |
| [`zenoh-pubsub-mock`](zenoh-pubsub-mock)   | `MockZenohSession`   | build + run | end-to-end pub/sub through the zenoh connector, no router needed |
| [`zenoh-pubsub-real`](zenoh-pubsub-real)   | `RealZenohSession`   | build only  | same shape, but over the real `zenoh::Session` (two-terminal recipe) |
| [`mqtt-pubsub-mock`](mqtt-pubsub-mock)     | `MockMqttSession`    | build + run | end-to-end pub/sub through the MQTT connector, no broker needed |
| [`mqtt-pubsub-real`](mqtt-pubsub-real)     | `RealMqttSession`    | build only  | real pub/sub over rumqttc against a bundled mosquitto (docker compose) |
| [`ethercat-stepper`](ethercat-stepper)     | `EthercrabBusDriver` | build only  | **recommended EtherCAT path** — ESI + netcfg `build.rs` codegen driving a Beckhoff EL7047 through generated typed drivers |
| [`ethercat-mock-loop`](ethercat-mock-loop) | `MockBusDriver`      | build + run | the raw-routing **escape hatch** — a 1 kHz loopback control loop wiring PDI bit-slice routing by hand (no codegen, no hardware) |
| [`ethercat-real-bus`](ethercat-real-bus)   | `EthercrabBusDriver` | build only  | drives a real Beckhoff EK1100 + EL1008 from a Linux host (Pi the canonical target) via ESI-generated typed drivers |

## Quick start

    cd examples/mqtt-zenoh-bridge && cargo run -- --ticks 5

That's the golden-path example: one executor bridging MQTT to Zenoh, no
broker or router needed. For a single-protocol pub/sub demo instead:

    cd examples/zenoh-pubsub-mock && cargo run

`zenoh-pubsub-real` needs two terminals; see its
[`README.md`](zenoh-pubsub-real/README.md).

## Debugging against in-tree changes

By default the examples consume the published crates.io versions, so
an in-tree fix is invisible to them. To flip every example to
`../../crates/taktora-*` path-deps:

    scripts/examples-local.sh on        # uncomment the patch blocks
    cd examples/zenoh-pubsub-mock && cargo run --release
    scripts/examples-local.sh off       # restore committed state

Status check (run from the repo root):

    scripts/examples-local.sh status

CI refuses to proceed if any example reports `on` — never commit while
the toggle is active.

## Versioning policy

Examples pin to `"0.1"` semver requirements. When release-plz publishes
a new major version of any `taktora-*` crate, the example manifests
here need a manual bump. That's intentional — these examples document
"what a downstream user writes against a released version," and
forcing the manual bump means we feel the same friction an external
user feels.

## Adding a new example

1. `cargo new examples/<name>` inside the repo (the workspace
   `exclude = ["examples"]` keeps Cargo from claiming it).
2. Add an empty `[workspace]` table to the new `Cargo.toml` so the
   crate opts out of any ancestor workspace (necessary for the
   `.claude/worktrees/...` development pattern).
3. Set `publish = false`.
4. Pin every `taktora-*` dependency to its current crates.io version.
5. Append the `# >>> taktora-examples-local-deps >>>` … `# <<<
   taktora-examples-local-deps <<<` marker block from any existing
   example (lists all 12 publishable workspace crates, commented out).
6. Add a `.gitignore` with `/target`. The repo-root `.gitignore` is
   rooted and does not cover subdirectory `target/` trees.
7. Write a `README.md` covering: what the example does, how to run
   it, and what to tweak.
8. If the example can run unattended (no router, no hardware), drop
   an empty `.runnable` file next to `Cargo.toml`. CI will then
   execute `cargo run --release -- --ticks 5` against it under a
   30-second watchdog.
9. Run `scripts/check-examples.sh` from the repo root to verify.
