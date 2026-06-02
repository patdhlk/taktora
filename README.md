# taktora

A Rust workspace exploring how to build a high-level execution framework and a
connector framework on top of [iceoryx2](https://github.com/eclipse-iceoryx/iceoryx2).

Three layered pieces:

- **`taktora-executor`** — items triggered by IPC, intervals, and request/response
  activity; sequential chains; parallel DAGs; signal/slot; lifecycle observability.
- **`taktora-connector-*`** — typed channels with codec-pluggable payloads,
  uniform connector health, and three reference connectors — EtherCAT (driving a
  SubDevice's process data), Zenoh (pub/sub + query/reply over a Zenoh session),
  and CAN/SocketCAN — all exposing the same plugin-facing `ChannelWriter` /
  `ChannelReader` types every other connector reuses.
- **EtherCAT build-time codegen toolchains** — turn vendor descriptions into
  strongly-typed Rust at build time, with zero runtime parsing: a *device-driver*
  toolchain (ESI XML → typed `EsiDevice` drivers) and a *network-config* toolchain
  (network YAML → PDO maps / routing tables for the connector).

> [!WARNING]
> **Personal experiment. Not meant for production.**
> The architecture is sound and the test suite is real, but the API has not
> stabilised, no version has been published, the `unsafe` story has not been
> independently audited, and there is no SLA, support, or backwards-compatibility
> guarantee. Use it to learn from, fork, or vendor in — not to ship.

**Specification:** [https://taktora.dev/](https://taktora.dev/) — built from `spec/` on every push to `main`.

## What's here

The workspace has grown to 23 library crates (plus per-crate `*-tests` siblings),
grouped by layer.

**Execution & runtime support**

| Crate | Purpose |
|---|---|
| [`taktora-executor`](crates/taktora-executor) | The execution core. Items, triggers, executor, runner, channels, services, chains, graphs, signal/slot, observer + execution monitor, optional thread tuning. |
| [`taktora-executor-tracing`](crates/taktora-executor-tracing) | `Observer` adapter forwarding executor lifecycle and user events to the global `tracing` subscriber. |
| [`taktora-bounded-alloc`](crates/taktora-bounded-alloc) | Static pre-allocated `#[global_allocator]` with hard caps on per-allocation size and total live blocks. `FEAT_0040`. |
| [`taktora-log`](crates/taktora-log) | Workspace logging facade (`log` crate facade with one-shot init, tracing-log bridge, console dev fallback). See `spec/requirements/logging.rst`. |
| [`taktora-log-dlt`](crates/taktora-log-dlt) | Pure-Rust AUTOSAR DLT R20-11 backend for `taktora-log`. Talks to a co-located COVESA `dlt-daemon` over UDS (default) or TCP. See `spec/architecture/logging.rst`. |
| [`taktora-replay`](crates/taktora-replay) | Empty placeholder for an eventual replay coordinator. Do not depend on it. |

**Connector framework + reference connectors**

| Crate | Purpose |
|---|---|
| [`taktora-connector-core`](crates/taktora-connector-core) | Framework-level traits and types shared by every connector — `Routing`, `ChannelDescriptor`, `PayloadCodec`, `ConnectorHealth` / `HealthEvent`, `ReconnectPolicy`, `ConnectorError`. `BB_0001`. |
| [`taktora-connector-transport-iox`](crates/taktora-connector-transport-iox) | iceoryx2-backed `ChannelWriter` / `ChannelReader` + `ConnectorEnvelope` POD wire format + `ServiceFactory`. `BB_0002`. |
| [`taktora-connector-codec`](crates/taktora-connector-codec) | `PayloadCodec` implementations. Ships `JsonCodec`; codec is compile-time-dispatched, so additional codecs are plug-in. `BB_0003`. |
| [`taktora-connector-host`](crates/taktora-connector-host) | `Connector` trait + `ConnectorHost` / `ConnectorGateway` builders + `HealthSubscription`. The seam at which protocol-specific connectors plug into an `Executor`. `BB_0005`. |
| [`taktora-connector-ethercat`](crates/taktora-connector-ethercat) | Reference EtherCAT connector. Pluggable `BusDriver` (mock or `ethercrab`), bit-slice PDI routing, gateway-side dispatcher that hops bytes between iceoryx2 and the SubDevice PDI each cycle. `BB_0030` / `FEAT_0041`. |
| [`taktora-connector-zenoh`](crates/taktora-connector-zenoh) | Reference Zenoh connector. Pluggable `ZenohSessionLike` back-end (mock or `zenoh::Session`), pub/sub + query/reply with timeout-correct sealed-queries handling, peer-count-driven health. `BB_0040` / `FEAT_0042`. |
| [`taktora-connector-can`](crates/taktora-connector-can) | Reference CAN / SocketCAN connector. `BB_0070` / `FEAT_0046`. |

**EtherCAT device-driver codegen** — ESI XML → typed drivers at build time (`FEAT_0050`)

| Crate | Purpose |
|---|---|
| [`taktora-ethercat-esi`](crates/taktora-ethercat-esi) | Parses EtherCAT ESI (slave information) XML into a faithful typed IR (identity, PDOs, sync managers, mailbox, distributed clocks, object dictionary). |
| [`taktora-ethercat-esi-rt`](crates/taktora-ethercat-esi-rt) | Runtime contract for generated drivers: the object-safe `EsiDevice` trait + runtime `EsiError`. The only crate generated code links against. |
| [`taktora-ethercat-esi-codegen`](crates/taktora-ethercat-esi-codegen) | Codegen layer: naming/collision policy, the `CodegenBackend` trait, and the `generate` entry point producing `proc-macro2` token streams. |
| [`taktora-ethercat-esi-codegen-ethercrab`](crates/taktora-ethercat-esi-codegen-ethercrab) | Concrete backend: emits per-device structs + `EsiDevice` impl (`decode_inputs`/`encode_outputs`), `Identity` consts, and a device registry. |
| [`taktora-ethercat-esi-build`](crates/taktora-ethercat-esi-build) | `build.rs` glue: glob ESI files → parse → generate → format → `$OUT_DIR`, encoding-aware (ISO-8859-1), with `rerun-if-changed`. |
| [`taktora-fieldbus-od-core`](crates/taktora-fieldbus-od-core) | Shared object-dictionary IR (`Identity`, `DataType`, dictionary entries) reused across fieldbus device descriptions. |

**EtherCAT network-config codegen** — network YAML → PDO maps / routing tables

| Crate | Purpose |
|---|---|
| [`taktora-ethercat-netcfg`](crates/taktora-ethercat-netcfg) | Parser + in-memory IR for the EtherCAT network-config YAML. |
| [`taktora-ethercat-netcfg-codegen`](crates/taktora-ethercat-netcfg-codegen) | Turns the netcfg IR into Rust source (PDO maps, routing tables) for the EtherCAT connector runtime. |
| [`taktora-ethercat-netcfg-build`](crates/taktora-ethercat-netcfg-build) | `build.rs` glue turning a `network.yaml` into a generated Rust module in `$OUT_DIR`. |
| [`taktora-ethercat-netcfg-cli`](crates/taktora-ethercat-netcfg-cli) | Command-line front end: expand a `network.yaml` to generated Rust source for inspection. |

## Executor quick start

```rust,no_run
use core::time::Duration;
use taktora_executor::{item_with_triggers, ControlFlow, Executor};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut exec = Executor::builder().worker_threads(0).build()?;
    exec.add(item_with_triggers(
        |d| { d.interval(Duration::from_secs(1)); Ok(()) },
        |_| { println!("tick"); Ok(ControlFlow::Continue) },
    ))?;
    exec.run()?;
    Ok(())
}
```

Press Ctrl-C; the loop exits cleanly. iceoryx2 catches the signal at the `Node`
level; the WaitSet returns it; the executor honors it. No extra signal-handler
plumbing on your side.

### What an `ExecutableItem` looks like

The unit of work the executor schedules. `declare_triggers` registers what
should wake it (subscriber arrivals, intervals, deadlines, server requests,
client responses, raw listeners). `execute` runs once per wake-up and returns
either `Continue`, `StopChain`, or an error.

```rust
struct MyTask { /* state */ }

impl taktora_executor::ExecutableItem for MyTask {
    fn declare_triggers(
        &mut self,
        d: &mut taktora_executor::TriggerDeclarer<'_>,
    ) -> Result<(), taktora_executor::ExecutorError> {
        d.interval(core::time::Duration::from_millis(100));
        Ok(())
    }
    fn execute(
        &mut self,
        _ctx: &mut taktora_executor::Context<'_>,
    ) -> taktora_executor::ExecuteResult {
        // do work
        Ok(taktora_executor::ControlFlow::Continue)
    }
}
```

Or the closure-based path: `taktora_executor::item(closure)` and
`item_with_triggers(declare_closure, execute_closure)`.

### Publishing options

`Publisher<T>` exposes three send paths with different cost/ergonomics
tradeoffs. iceoryx2's zero-copy promise holds across the wire in every case
(the receiver always gets a reference to shared memory); the differences are
on the sender's side:

| Method | Sender-side cost | When to reach for it |
|---|---|---|
| `send_copy(value)` | One move into shm | Tiny POD payloads (`u64`, small structs). Simplest. |
| `loan_send(\|t\| ...)` | `T::default()` write + in-place mutation | Medium types where `Default` is cheap. |
| `loan(\|slot\| ...)` | None — closure constructs directly in shm | Large types or types without a sensible `Default`. |

For large types use `loan` with `MaybeUninit::write(value)` or iceoryx2's
`placement_default!` macro to get the full zero-copy benefit. The
[`loan_demo`](crates/taktora-executor/examples/loan_demo.rs) example sends 1 KB
payloads constructed entirely in shared memory.

### Composition

- `Executor::add(item)` — single item dispatched as one pool job.
- `Executor::add_chain([head, mid, tail])` — sequential walk; head's triggers
  gate the chain; `Ok(StopChain)` or `Err` from any item aborts the rest.
- `Executor::add_graph().vertex(...).edge(...).root(...).build()` — a DAG.
  Vertices run in parallel on the executor's thread pool when their predecessors
  complete. The root's triggers gate the graph.
- `wrap_with_condition(item, predicate)` — gate any item on a runtime check.
- `signal_slot::pair(&mut exec, topic)` — pre-built `ExecutableItem`s wrapping
  a `Channel<T>` for chain composition with `before_send`/`after_recv` hooks.

See `crates/taktora-executor/examples/` for runnable variants of each.

### Observability

- **`Observer`** trait — `on_executor_up/down/error`, `on_app_start/stop/error`,
  `on_send_event`. No-op default impls; non-blocking. The
  `taktora-executor-tracing` crate ships a ready-made adapter to the `tracing`
  ecosystem.
- **`ExecutionMonitor`** trait — `pre_execute(task, at)` /
  `post_execute(task, at, took, ok)`. Raw timestamps; build expectations on top.

Both are configured via `ExecutorBuilder::observer(...)` /
`ExecutorBuilder::monitor(...)`.

## Connector framework

A protocol-agnostic surface for getting data into and out of an `Executor`.
Each concrete connector implements the `Connector` trait from
`taktora-connector-host`:

```rust,ignore
pub trait Connector: Send + 'static {
    type Routing: Routing;
    type Codec: PayloadCodec;

    fn name(&self) -> &str;
    fn health(&self) -> ConnectorHealth;
    fn subscribe_health(&self) -> HealthSubscription;
    fn register_with(&mut self, executor: &mut Executor) -> Result<(), ConnectorError>;
    fn create_writer<T, const N: usize>(&self, descriptor: &ChannelDescriptor<Self::Routing, N>)
        -> Result<ChannelWriter<T, Self::Codec, N>, ConnectorError>
    where T: serde::Serialize;
    fn create_reader<T, const N: usize>(&self, descriptor: &ChannelDescriptor<Self::Routing, N>)
        -> Result<ChannelReader<T, Self::Codec, N>, ConnectorError>
    where T: serde::de::DeserializeOwned;
}
```

The plugin side calls `create_writer` / `create_reader` to get typed handles.
Each handle is backed by an iceoryx2 service; payloads are codec-encoded
on `send` and decoded on `try_recv`. The connector's gateway side moves bytes
between iceoryx2 and the protocol-specific I/O surface.

### EtherCAT reference connector

[`taktora-connector-ethercat`](crates/taktora-connector-ethercat) is the first
concrete protocol. After `register_with`, calling `ChannelWriter::send(value)`
on the plugin side causes a bit to flip on the addressed SubDevice's PDI
each cycle:

1. Plugin's `ChannelWriter::send` encodes the value via the connector's codec
   and publishes it on an iceoryx2 service.
2. Gateway-side dispatcher drains the publish, runs `pdi::write_routing` to
   place the bytes at the channel's `EthercatRouting` (subdevice address +
   bit offset + bit length), and the cycle's `tx_rx` ships the PDI out.
3. On the inbound leg, the dispatcher reads the SubDevice's inputs slice,
   slices out the routing's bits via `pdi::read_routing`, and publishes the
   raw bytes back on a paired iceoryx2 service.
4. Plugin's `ChannelReader::try_recv` decodes those bytes.

The dispatcher is driven by a pluggable `BusDriver`:

- **`MockBusDriver`** — programmable working-counter sequences, configurable
  per-SubDevice PDI buffers, optional loopback. End-to-end tests (`TEST_0220`
  / `TEST_0221` / `TEST_0222`) exercise the full iceoryx2 ↔ PDI ↔ iceoryx2
  hop in CI without hardware.
- **`EthercrabBusDriver`** (under the `bus-integration` feature) — wraps
  `ethercrab::MainDevice`, spawns the `tx_rx_task` on the gateway's tokio
  runtime, drives PRE-OP → SAFE-OP → OP bring-up, applies the configured
  PDO mapping via SDO writes to `0x1C12` / `0x1C13`. Awaits hardware
  (`ETHERCAT_TEST_NIC`) for the real-bus tests.

Each plugin-side channel is opened on `"{descriptor.name()}.out"` (outbound,
plugin → gateway → SubDevice outputs PDI) or `"{descriptor.name()}.in"`
(inbound, SubDevice inputs PDI → gateway → plugin). Adjacent routing slices
on the same SubDevice are preserved across writes via bit-level
read-modify-write.

### Zenoh reference connector

[`taktora-connector-zenoh`](crates/taktora-connector-zenoh) is the second
concrete protocol. It exposes pub/sub on the standard `ChannelWriter` /
`ChannelReader` handles and query/reply through the connector's
`ZenohQuerier` / `ZenohQueryable` handles, all over a single Zenoh session:

1. Plugin's `ChannelWriter::send` encodes the value via the connector's
   codec and publishes it on an iceoryx2 service.
2. Gateway-side dispatcher drains the publish, applies the channel's
   `ZenohRouting` (key expression, congestion control, reliability,
   priority), and forwards via the back-end session.
3. Declared subscribers feed inbound bytes back through the connector to
   `ChannelReader::try_recv`.
4. Query traffic flows through `ZenohQuerier::query` and
   `ZenohQueryable::reply`, with a `sealed_queries` sidecar that
   coordinates the timeout/late-reply race so a reply arriving after the
   query's deadline is dropped instead of leaking to the plugin.

The session back-end is pluggable:

- **`MockZenohSession`** — in-process programmable session. Exposes a
  `peer_count` knob so health-watcher tests (`REQ_0442`) can drive
  `Healthy` ↔ `Degraded` transitions deterministically and verify the
  query lifecycle without a running router.
- **`RealZenohSession`** (under the `zenoh-integration` feature) — wraps
  `zenoh::Session` (zenoh 1.x), owns its own tokio runtime, and enables
  `transport_tcp` so it can open `tcp/...` locators.

`ZenohHealthMonitor` polls the back-end's peer count against a
configurable `min_peers` threshold and emits `HealthEvent`s through the
same `subscribe_health` surface every connector exposes.

## EtherCAT codegen toolchains (build-time)

Two toolchains turn vendor descriptions into strongly-typed Rust entirely in
`build.rs`, with zero runtime parsing — consumers `include!` the generated module
and ship no XML/YAML.

- **Device-driver codegen (`FEAT_0050`).** `taktora-ethercat-esi-build` reads a
  vendor ESI XML file and generates per-device structs implementing the
  object-safe `EsiDevice` trait (`taktora-ethercat-esi-rt`): typed
  `decode_inputs` / `encode_outputs` over the SubDevice's process image, an
  `Identity` const per device, and a registry for identity-based dispatch. The
  parser copes with real-world ESI quirks (localized `<Name>`, CDATA, ISO-8859-1,
  vendor data types). Real Beckhoff terminals (EL1008 / EL2004 / EL3602, …)
  generate compiling, bit-exact drivers; the [`ethercat-real-bus`](examples/ethercat-real-bus)
  example drives EL1008/EL2004 through generated drivers, validated on real hardware.
- **Network-config codegen.** `taktora-ethercat-netcfg-build` turns a network
  `.yaml` into the PDO maps and routing tables the EtherCAT connector consumes,
  with a `cargo`-style CLI (`taktora-ethercat-netcfg-cli`) for inspection.

## Threading

Single executor-owned worker pool (M1 model). The thread that calls
`Executor::run()` becomes the WaitSet driver; pool workers run `execute()`.
For parallel graphs, use `worker_threads(N)` with `N >= 2`.

`Runner::new(exec, RunnerFlags::empty())` hosts the executor on a dedicated
OS thread; `Runner::stop()` joins it and re-throws any item error.

Connectors that need an async I/O loop (e.g. the EtherCAT gateway around
`ethercrab`) own their own tokio runtime internally — it never leaks into
the WaitSet thread.

## Cargo features

| Flag             | Crate | Default | Effect                                     |
|------------------|---|---------|--------------------------------------------|
| `tracing`        | `taktora-executor` | off     | Add the `tracing` crate as a dependency for adapter integrations. |
| `thread_attrs`   | `taktora-executor` | off     | Core-affinity, thread name prefix, and (Linux) `SCHED_FIFO` priority on the executor's worker pool. |
| `json`           | `taktora-connector-codec` | **on** | `JsonCodec` via `serde_json`. |
| `bus-integration`| `taktora-connector-ethercat` | off | Pull `ethercrab` and expose `EthercrabBusDriver`. Off by default so consumers that only want the framework types and pure-logic helpers don't pull ethercrab's transitive dependencies. |
| `zenoh-integration`| `taktora-connector-zenoh` | off | Pull `zenoh` 1.x (with `transport_tcp` enabled) and expose `RealZenohSession`. Off by default so consumers that only want the framework types and `MockZenohSession` don't pull zenoh's transitive dependencies. |

iceoryx2 itself handles SIGINT/SIGTERM natively — no `ctrlc` feature is
needed and the loop exits cleanly on either signal.

## Detecting dropped notifications

When a publisher sends, iceoryx2 wakes each attached listener via a per-listener
Unix datagram socket. If the listener's kernel socket buffer is full, that
specific notification is dropped (the **data** still goes through the pub/sub
channel reliably; only the wakeup is lost). iceoryx2 logs a verbose
`FailedToDeliverSignal` warning when this happens.

**This usually means the consumer is falling behind.** Either it can't drain
fast enough, or the producer is bursting faster than the listener's socket
buffer can absorb. Lost wakeups can usually be tolerated (the listener will
still wake from a *previous* pending notification, drain everything, and
catch up), but if you have a deadline-sensitive consumer or zero-buffered
event semantics, every drop matters.

The publisher's send methods return `NotifyOutcome` so callers can detect
this programmatically without parsing logs:

```rust,no_run
# use taktora_executor::Publisher;
# fn run(publisher: Publisher<u64>) -> Result<(), Box<dyn std::error::Error>> {
let outcome = publisher.send_copy(42_u64)?;
if !outcome.delivered_to_any_listener() {
    // No listener received the wakeup. Either no subscribers are attached
    // (normal during startup), or every subscriber's socket was full.
    eprintln!("warn: send dropped, listeners_notified={}", outcome.listeners_notified);
}
# Ok(()) }
```

If you want to silence iceoryx2's verbose logging anyway (e.g. in production),
call `iceoryx2::prelude::set_log_level(LogLevel::Error)` once at startup. But
inspect `NotifyOutcome::listeners_notified` first — silencing the log without
checking the return is how dropped wakeups become silent bugs.

## Examples

Two kinds, depending on what you want to see.

### Per-crate examples (cargo workspace)

Tight, focused demos that live inside their crate's `examples/`
directory and build from the workspace itself:

| Example | What it shows |
|---|---|
| `cargo run -p taktora-executor --example interval_loop`   | one tick per second; Ctrl-C to exit |
| `cargo run -p taktora-executor --example pubsub_pipeline` | producer + consumer over a `Channel<u64>` |
| `cargo run -p taktora-executor --example diamond_graph`   | 4-vertex DAG fired by an interval |
| `cargo run -p taktora-executor --example signal_slot`     | signal/slot pair driven by a chain |
| `cargo run -p taktora-executor --example loan_demo`       | zero-copy 1 KB payloads via `Publisher::loan` |

### Integration examples (standalone crates)

Cross-crate mini-apps under [`examples/`](examples/) that depend on the
published `taktora-*` crates from crates.io. Each is its own Cargo
crate with its own `Cargo.lock`, so they read like what an external
user would write:

| Example | What it shows |
|---|---|
| [`zenoh-pubsub-mock`](examples/zenoh-pubsub-mock)   | executor + zenoh connector (`MockZenohSession`), in-process pub/sub |
| [`zenoh-pubsub-real`](examples/zenoh-pubsub-real)   | same shape over a real `zenoh::Session`; two-terminal peer-to-peer |
| [`ethercat-mock-loop`](examples/ethercat-mock-loop) | 1 kHz control loop over the EtherCAT connector with PDI bit-slice routing through `MockBusDriver` |
| [`ethercat-wago-coupler`](examples/ethercat-wago-coupler) | mirrors a WAGO 750-430's 8 inputs to a 750-530's 8 outputs through a 750-354 coupler over a real NIC |
| [`ethercat-real-bus`](examples/ethercat-real-bus)   | drives a real EK1100 + EL1008 + EL2004 over a Linux NIC (Pi-friendly), decoding inputs / encoding outputs via **ESI-generated typed drivers** |

See [`examples/README.md`](examples/README.md) for the full index and
the local-paths toggle (debugging an in-tree change against an
example).

## Building

Workspace builds on stable Rust (edition 2024, MSRV 1.85). iceoryx2 0.8.x
is the underlying IPC layer.

```bash
cargo build --workspace
cargo test  --workspace --all-features -- --test-threads=1
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Pre-push hooks (cargo-clippy, cargo-doc, complexity-gate) mirror the CI jobs.
Install with `pre-commit install && pre-commit install --hook-type pre-push`.
Per-tool notes:

- **Complexity gate (optional locally):** install once with
  `cargo install --locked rust-code-analysis-cli` (the `--locked` is required;
  a plain `cargo install` fails to build on current toolchains) so the
  `complexity-gate` pre-push hook runs. Without it the hook self-skips; CI
  enforces it regardless.

Tests run single-threaded in CI because each test creates its own iceoryx2
service in shared memory (parallel runs would contend on the same names) and
the `CountingAllocator` used by the zero-alloc tests is process-wide.

## Status

This is **pre-1.0 personal experiment code.** Concretely:

- The API has not been audited by anyone other than the author.
- The `unsafe` blocks (cross-thread send of iceoryx2 ports, raw-pointer
  dispatch in the WaitSet callback, raw-pointer envelope construction in
  the connector transport's zero-copy publish path) are documented but
  have not been reviewed by an `unsafe`-Rust expert or run under Miri.
- Several known polish items remain (see the design notes for the punch
  list); none are correctness-blocking, but the API surface should be
  considered unstable until they're addressed.
- iceoryx2 0.8.x is itself pre-1.0 and changes shape between versions;
  this workspace is pinned to 0.8.x and will need adaptation for later
  releases.
- The EtherCAT connector's real-bus path (`bus-integration` feature) is
  compile-checked only — hardware tests run under `ETHERCAT_TEST_NIC`
  and are not part of the default CI matrix.
- The Zenoh connector's real-session path (`zenoh-integration` feature)
  follows the same model: the test matrix exercises `MockZenohSession`
  in CI; `RealZenohSession` is compile-checked and validated against a
  running router out-of-band.
- The safety concept under [`spec/safety/`](spec/safety) is **SEooC
  sketch-level**. It captures an assumed item, an illustrative HARA,
  two assumed safety goals, five AFSRs, ten TSRs allocated across the
  workspace, a Freedom-From-Interference argument, and a nine-item
  Assumption-of-Use contract. ASIL D is claimed via ISO 26262-9 §5
  decomposition (taktora as `ASIL B(D)`, a diverse integrator-supplied
  monitor as the second `ASIL B(D)`) — the independence argument is
  **not closed by taktora** and the whole concept has not been assessed.
- No version has been published to crates.io. There is no support, no
  release cadence, no SLA, and no backwards-compatibility guarantee.

If any of those caveats matter for your use case, **don't ship it**.

Read the source, fork it, vendor it, or treat it as a worked example for
how to wire iceoryx2 into a higher-level execution framework — but don't
mistake it for a maintained library.

## License

Apache-2.0 OR MIT, at your option.
