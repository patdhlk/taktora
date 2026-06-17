# Adding a new connector

This guide walks through everything needed to add a new protocol connector
to the `taktora-connector` framework — from the spec/RFC paperwork, through
the crate scaffold and the framework contract, down to the build/test/lint
loop that CI enforces.

It is grounded in the two reference connectors that ship today:

- **`taktora-connector-can`** — the smallest *complete* connector. No
  request/response, no remote peers. **Copy this one** when starting a
  new connector.
- **`taktora-connector-zenoh`** — a richer connector with queries
  (`create_querier` / `create_queryable`). Look here for protocol-specific
  affordances that live *beside* the `Connector` trait.

Throughout, the running example is a hypothetical **MQTT** connector
(`taktora-connector-mqtt`). Substitute your protocol's name for `mqtt`/`Mqtt`.

---

## 1. How a connector works

### 1.1 The big picture

A connector is the bridge between an application and one external protocol
(MQTT, EtherCAT, CAN, Zenoh, …). It has two halves that talk over iceoryx2
shared memory:

```
   ┌─────────────────────────┐         ┌──────────────────────────────┐
   │  application process     │         │  gateway (in-process task OR  │
   │                          │         │  separate process)            │
   │  ChannelWriter<T,C,N> ───┼─.out──► │  raw reader ─► tokio sidecar  │
   │  ChannelReader<T,C,N> ◄──┼─.in───  │  raw writer ◄─ protocol stack │
   │                          │ iceoryx2│  (rumqttc / ethercrab /       │
   │  (WaitSet / executor     │   SHM   │   socketcan / zenoh::Session) │
   │   thread)                │         │  (tokio runtime)              │
   └─────────────────────────┘         └──────────────────────────────┘
```

- The **plugin side** (the `Connector` impl the app holds) does *no protocol
  I/O*. It only creates iceoryx2 channels and hands the app typed
  `ChannelWriter` / `ChannelReader` handles.
- The **gateway side** owns a tokio runtime and the real protocol stack. A
  per-connector *dispatcher loop* drains outbound iceoryx2 traffic into the
  protocol and publishes inbound protocol traffic back onto iceoryx2.
- Both deployment shapes are supported (ADR_0003): the gateway can run as a
  tokio task inside the app process, or as a separate process.

### 1.2 The three framework layers

| Crate | Role | You depend on it, never edit it |
|-------|------|--------------------------------|
| `taktora-connector-core` | Pure traits/types: `Routing`, `ChannelDescriptor<R, N>`, `PayloadCodec`, `ConnectorHealth`/`HealthEvent`/`HealthMonitor`, `ReconnectPolicy`/`ExponentialBackoff`, `ConnectorError` (BB_0001) | ✅ |
| `taktora-connector-transport-iox` | The wire format `ConnectorEnvelope<N>`, the concrete `ChannelWriter<T, C, N>` / `ChannelReader<T, C, N>`, and `ServiceFactory` that binds iceoryx2 (BB_0002) | ✅ |
| `taktora-connector-host` | The `Connector` trait itself, `HealthSubscription`, and `ConnectorHost` / `ConnectorGateway` composition + builder (BB_0005) | ✅ |
| `taktora-connector-codec` | Concrete `PayloadCodec`s, e.g. `JsonCodec` (BB_0003) | ✅ (or bring your own codec) |

Your new crate `taktora-connector-mqtt` implements the `Connector` trait
*against* these and adds the protocol-specific machinery.

### 1.3 The contract you must satisfy

The whole framework contract is one trait, in
`crates/taktora-connector-host/src/connector.rs`:

```rust
pub trait Connector: Send + 'static {
    type Routing: Routing;          // your typed routing struct
    type Codec: PayloadCodec;       // usually a generic `C`

    fn name(&self) -> &str;
    fn health(&self) -> ConnectorHealth;
    fn subscribe_health(&self) -> HealthSubscription;

    fn register_with(&mut self, executor: &mut Executor) -> Result<(), ConnectorError>;

    fn create_writer<T, const N: usize>(
        &self,
        descriptor: &ChannelDescriptor<Self::Routing, N>,
    ) -> Result<ChannelWriter<T, Self::Codec, N>, ConnectorError>
    where T: serde::Serialize;

    fn create_reader<T, const N: usize>(
        &self,
        descriptor: &ChannelDescriptor<Self::Routing, N>,
    ) -> Result<ChannelReader<T, Self::Codec, N>, ConnectorError>
    where T: serde::de::DeserializeOwned;
}
```

Key facts that fall out of this:

- The trait is **`Send + 'static` but not `Sync`** and **not dyn-compatible**
  (it has associated types and generic methods). The host registers one
  concrete connector at a time. Concrete connectors hold an `Arc` to shared
  state and expose `&self` methods.
- **`N` is a compile-time const generic** carried by `ChannelDescriptor<R, N>`
  all the way into the channel handles. A writer and reader with mismatched
  `N` cannot type-check (REQ_0201, REQ_0205).
- **The codec is a type parameter**, monomorphised — chosen at compile time
  (REQ_0211).
- **Health is observable**: `health()` is a cheap snapshot; `subscribe_health()`
  returns an event stream following the `ConnectorHealth` state machine
  (ARCH_0012): `Connecting → {Up, Degraded, Down}` and back.

### 1.4 The wire format

Everything crosses iceoryx2 as a POD `ConnectorEnvelope<N>` (BB_0002,
`crates/taktora-connector-transport-iox/src/envelope.rs`):

```rust
#[repr(C)]
pub struct ConnectorEnvelope<const N: usize> {
    pub sequence_number: u64,   // per-(publisher,channel) monotonic, from 0 (REQ_0202)
    pub timestamp_ns: u64,      // ns since UNIX epoch at loan time (REQ_0203)
    pub correlation_id: [u8; 32],// passive carrier for request/response (REQ_0204)
    pub payload_len: u32,       // valid bytes in payload, <= N
    pub reserved: u32,          // connector-defined metadata slot
    pub payload: [u8; N],       // codec writes directly here (zero-copy loan)
}
```

You almost never touch this directly — `ServiceFactory::create_writer` /
`create_reader` build `ChannelWriter`/`ChannelReader` that own it. The codec
encodes straight into `payload[..]` via iceoryx2's `loan_uninit()` path
(REQ_0205).

---

## 2. Before you write code: the spec loop

Per [CONTRIBUTING.md](../../CONTRIBUTING.md), a new connector is a
"substantial design" and goes through the RFC + sphinx-needs loop. The spec
lives under `spec/` and is published to <https://taktora.dev/>.

1. **Open an RFC issue** using the `03-rfc.yml` template (label
   `kind:rfc`; add `area:connector-<proto>`).
2. **Draft the requirements** under
   `spec/requirements/connector/<proto>.rst`:
   - One `feat::` (e.g. `FEAT_00xx`) that `:satisfies: FEAT_0030` (the
     connector-framework umbrella).
   - Child `req::` clauses for routing, options, health policy,
     reconnect-vs-stack-internal, bridge saturation behaviour, and explicit
     **anti-goals** (what you deliberately will *not* do — see the existing
     `cross-cutting.rst` for the pattern, e.g. REQ_0290–REQ_0296).
   - Set `:status: draft` on every new need.
3. **Draft the architecture** under
   `spec/architecture/connector/building-blocks.rst`:
   - A top-level `spec::` building block `BB_00x0` for the crate, plus
     sub-blocks for the plugin connector, the gateway, the tokio bridge, and
     any protocol-specific helper (mirror BB_0070–BB_0075 for CAN).
   - Add an `impl::` directive in
     `spec/architecture/connector/implementations.rst` that locks the public
     surface and lists the REQs it refines and TESTs that cover it.
   - Most design decisions are already locked protocol-neutrally in
     ADR_0003–ADR_0009; add a new `arch-decision::` only for a
     protocol-specific call (see ADR_0040–ADR_0043 for Zenoh's queries).
4. **Draft verification** under `spec/verification/connector/<proto>.rst`:
   one `test::` (`TEST_00xx`) per behaviour, each `:verifies:` a REQ.
5. Discuss in the RFC, then **promote `:status:` from `draft` to `open`**
   once accepted.

> Traceability gate: when you later flip a `req::` to
> `:status: implemented`, sphinx-build `-W` **requires** it to carry
> `:links:` to its building block *and* its test. Verify the committed tree
> builds before claiming done (see §13).

---

## 3. Step-by-step implementation

> The fastest path is to **copy `crates/taktora-connector-can`** and rename.
> The steps below explain each file so you know what to keep, what to change,
> and what is pure boilerplate.

### Step 1 — Scaffold the two crates

Every connector is **two** crates: the published crate and a
`publish = false` tests crate. The split is mandatory and exists to dodge the
release-plz dev-dep ordering trap: internal `dev-dependencies` in a published
crate break the publish topological sort.

```bash
mkdir -p crates/taktora-connector-mqtt/src
mkdir -p crates/taktora-connector-mqtt-tests/tests
```

### Step 2 — Wire the crates into the workspace

Edit the **root `Cargo.toml`**:

1. Add both crates to `[workspace.members]`:
   ```toml
   "crates/taktora-connector-mqtt",
   "crates/taktora-connector-mqtt-tests",
   ```
2. Add the published crate to `[workspace.dependencies]` so sibling crates
   (tests, examples) reference it by `{ workspace = true }`:
   ```toml
   taktora-connector-mqtt = { path = "crates/taktora-connector-mqtt", version = "0.1.0" }
   ```

### Step 3 — `crates/taktora-connector-mqtt/Cargo.toml`

Inherit all metadata from the workspace. Gate the real protocol library
behind a default-off `*-integration` feature so a plain `cargo build` never
pulls the heavy transitive deps. (CONTRIBUTING.md §"Coding conventions":
*ship a mock back-end alongside the real one, gate the real one behind a
`*-integration` Cargo feature*.)

```toml
[package]
name        = "taktora-connector-mqtt"
version     = "0.1.0"
edition     = { workspace = true }
rust-version = { workspace = true }
license     = { workspace = true }
repository  = { workspace = true }
authors     = { workspace = true }
homepage    = { workspace = true }
readme      = { workspace = true }
keywords    = { workspace = true }
categories  = { workspace = true }
description = "MQTT reference connector for the taktora-connector framework. Implements BB_00x0 (FEAT_00xx)."

[features]
default = []
# Pulls the real protocol stack. Default-off; the mock back-end is always built.
rumqttc-integration = ["dep:rumqttc"]

[dependencies]
# Framework — every connector has these four.
taktora-connector-core          = { workspace = true }
taktora-connector-transport-iox = { workspace = true }
taktora-connector-host          = { workspace = true }
taktora-executor                = { workspace = true }
# Shared infra.
iceoryx2          = { workspace = true }
crossbeam-channel = { workspace = true }
tokio             = { workspace = true }
serde             = { workspace = true, features = ["std"] }
thiserror         = { workspace = true }
tracing           = { workspace = true }
# Protocol stack — optional, behind the feature above.
rumqttc = { version = "0.24", optional = true }

[lints]
workspace = true   # or copy the per-crate [lints.rust]/[lints.clippy] block from a sibling connector
```

> **Platform-gated stacks (CAN/EtherCAT pattern):** if the real backend is
> Linux-only, additionally `cfg`-gate it:
> ```toml
> [target.'cfg(target_os = "linux")'.dependencies]
> socketcan = { version = "3", optional = true, default-features = false, features = ["tokio"] }
> ```
> Remember the *Linux-gated clippy blind spot*: `cfg`-gated files escape macOS
> clippy — run CI's clippy line on the target platform before merging.

### Step 4 — `routing.rs` (protocol-specific)

Define a typed routing struct that carries everything needed to address one
channel, validate the protocol's identifiers at construction, and implement
the `Routing` marker trait (it has **no methods** — it only collects the
`Clone + Send + Sync + Debug + 'static` bounds, REQ_0222/REQ_0224).

```rust
use taktora_connector_core::Routing;

/// Validated MQTT topic (no wildcards on the publish side, etc.).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MqttTopic(String);

impl MqttTopic {
    pub fn new(topic: impl Into<String>) -> Result<Self, TopicError> {
        let topic = topic.into();
        if topic.is_empty() {
            return Err(TopicError::Empty);
        }
        // ... protocol-specific validation ...
        Ok(Self(topic))
    }
    pub fn as_str(&self) -> &str { &self.0 }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QoS { AtMostOnce, AtLeastOnce, ExactlyOnce }

#[derive(Debug, Clone)]
pub struct MqttRouting {
    pub topic: MqttTopic,
    pub qos: QoS,
    pub retained: bool,
}

impl MqttRouting {
    pub const fn new(topic: MqttTopic, qos: QoS) -> Self {
        Self { topic, qos, retained: false }
    }
}

impl Routing for MqttRouting {}
```

Look at `can/src/routing.rs` for the bounded-identifier pattern (`CanIface`,
`CanId`) and `zenoh/src/routing.rs` for QoS-rich routing with a validated
key-expression.

### Step 5 — `options.rs` (protocol-specific)

A typed builder for connector-wide configuration (broker URL, credentials,
tokio worker threads, and the **bridge capacities / drop thresholds** that
bound back-pressure). Clamp zero values to a sane minimum in `build()`. Copy
the shape from `can/src/options.rs`.

### Step 6 — `health.rs` (mostly boilerplate)

Wrap `taktora_connector_core::HealthMonitor` behind a small struct that adds
subscriber fan-out over a `crossbeam_channel`. Each call to `subscribe()`
returns an independent receiver (REQ_0847 — health streams are independent
broadcasts, not load-balanced). The `current()` / `subscribe()` / a
`transition_*` method are all the connector needs. Copy `can/src/health.rs`
(or `zenoh/src/health.rs`); for a multi-endpoint protocol, do the per-endpoint
worst-of aggregation that `CanHealthMonitor` does.

### Step 7 — Backend trait + mock + real (protocol-specific)

Define one async trait abstracting the protocol I/O so the connector is
generic over `MockMqttSession` (always built, used by tests) and
`RealMqttSession` (behind `rumqttc-integration`). This is the CAN
`CanInterfaceLike` / Zenoh `ZenohSessionLike` pattern:

```rust
pub trait MqttSessionLike: Send {
    // protocol-specific async ops the dispatcher calls, e.g.
    async fn publish(&self, topic: &str, qos: QoS, payload: &[u8]) -> Result<(), MqttIoError>;
    async fn next_message(&mut self) -> Result<MqttMessage, MqttIoError>;
}
```

Put `MockMqttSession` in `mock.rs` (in-process loopback, **never**
feature-gated), and `RealMqttSession` in `real.rs` gated by
`#[cfg(feature = "rumqttc-integration")]`.

### Step 8 — Gateway, dispatcher, bridge, registry (protocol-specific internals)

These are not framework types — they are the connector's own machinery. Copy
the shapes from CAN:

- **`gateway.rs`** — owns the tokio `Runtime`; hands out a `Handle`; on
  `Drop` shuts the runtime down within a budget (5s clean-exit, REQ_0241).
- **`bridge.rs`** — bounded `crossbeam_channel` pairs between the WaitSet
  thread and the tokio side. Outbound applies back-pressure
  (`ConnectorError::BackPressure`); inbound drops oldest past a threshold and
  counts drops (feeds a `Degraded`/drop `HealthEvent`).
- **`registry.rs`** — maps descriptor name → routing + the raw iceoryx2
  endpoints the dispatcher drives. Iteration order stable, alloc-free.
- **`dispatcher.rs`** — the tokio loop: drain outbound bridge → protocol
  publish; protocol receive → inbound bridge → iceoryx2 publish; classify
  errors into health transitions; reconnect if the stack surfaces raw connect
  events (use `ExponentialBackoff` from core, REQ_0232/REQ_0233 — *unless* the
  stack reconnects internally, like Zenoh, ADR_0041).

### Step 9 — `connector.rs` (the `Connector` impl)

This is the only file that touches the framework contract. Pattern (verified
against `can/src/connector.rs`):

```rust
use std::sync::Arc;
use iceoryx2::node::Node;
use iceoryx2::prelude::{ipc, NodeBuilder};
use taktora_connector_core::{ChannelDescriptor, ConnectorError, ConnectorHealth};
use taktora_connector_host::{Connector, HealthSubscription};
use taktora_connector_transport_iox::{ChannelReader, ChannelWriter, ServiceFactory};
use taktora_executor::Executor;

pub struct MqttState { /* options, Arc<MqttHealthMonitor>, registry, stop flag */ }

pub struct MqttConnector<S, C>
where
    S: MqttSessionLike,
{
    state: Arc<MqttState>,
    codec: C,
    node: Arc<Node<ipc::Service>>,
    gateway: MqttGateway,
    session_slot: std::sync::Mutex<Option<S>>, // moved into dispatcher on register_with
}

impl<S, C> MqttConnector<S, C>
where
    S: MqttSessionLike + 'static,
    C: taktora_connector_core::PayloadCodec + Clone + Send + Sync + 'static,
{
    pub fn new(state: Arc<MqttState>, session: S, codec: C) -> Result<Self, ConnectorError> {
        let node = NodeBuilder::new()
            .create::<ipc::Service>()
            .map_err(ConnectorError::stack)?;
        let gateway = MqttGateway::new(state.options().clone())
            .map_err(ConnectorError::stack)?;
        Ok(Self { state, codec, node: Arc::new(node), gateway,
                  session_slot: std::sync::Mutex::new(Some(session)) })
    }

    fn factory(&self) -> ServiceFactory<'_> { ServiceFactory::new(&self.node) }
}

impl<S, C> Connector for MqttConnector<S, C>
where
    S: MqttSessionLike + 'static,
    C: taktora_connector_core::PayloadCodec + Clone + Send + Sync + 'static,
{
    type Routing = MqttRouting;
    type Codec = C;

    fn name(&self) -> &str { "mqtt" }
    fn health(&self) -> ConnectorHealth { self.state.health().current() }
    fn subscribe_health(&self) -> HealthSubscription {
        HealthSubscription::new(self.state.health().subscribe())
    }

    fn register_with(&mut self, executor: &mut Executor) -> Result<(), ConnectorError> {
        // 1. Take the session out of session_slot (error if called twice).
        // 2. gateway.handle().spawn(dispatcher_loop(...))  — protocol I/O.
        // 3. executor.add(<heartbeat ExecutableItem>) — keeps the WaitSet ticking.
        Ok(())
    }

    fn create_writer<T, const N: usize>(
        &self,
        descriptor: &ChannelDescriptor<Self::Routing, N>,
    ) -> Result<ChannelWriter<T, Self::Codec, N>, ConnectorError>
    where T: serde::Serialize {
        let routing = descriptor.routing().clone();
        // validate routing + that N fits the protocol's max payload here.
        let svc_name = format!("{}.out", descriptor.name());
        let plugin_desc = ChannelDescriptor::<Self::Routing, N>::new(svc_name.clone(), routing)?;
        let writer = self.factory().create_writer::<T, _, _, N>(&plugin_desc, self.codec.clone())?;
        // Open the gateway-side raw reader on the same service + register in the registry.
        let _raw = self.factory().create_raw_reader_named::<N>(&svc_name)?;
        // ... store binding so the dispatcher drains it ...
        Ok(writer)
    }

    fn create_reader<T, const N: usize>(
        &self,
        descriptor: &ChannelDescriptor<Self::Routing, N>,
    ) -> Result<ChannelReader<T, Self::Codec, N>, ConnectorError>
    where T: serde::de::DeserializeOwned {
        let routing = descriptor.routing().clone();
        let svc_name = format!("{}.in", descriptor.name());
        let plugin_desc = ChannelDescriptor::<Self::Routing, N>::new(svc_name.clone(), routing)?;
        let reader = self.factory().create_reader::<T, _, _, N>(&plugin_desc, self.codec.clone())?;
        let _raw = self.factory().create_raw_writer_named::<N>(&svc_name)?;
        // ... wire the protocol subscription to publish onto this service ...
        Ok(reader)
    }
}
```

Conventions to keep:

- Service naming is **`"{descriptor.name()}.out"`** for writers and
  **`"{descriptor.name()}.in"`** for readers (each logical channel is two
  one-direction iceoryx2 services).
- The session/drivers are moved out of a `Mutex<Option<…>>` slot the *first*
  time `register_with` runs; a second call returns a stack error.
- `register_with` must `executor.add(...)` at least a heartbeat
  `ExecutableItem` so the host's WaitSet keeps cycling.

### Step 10 — `lib.rs` re-exports

Mirror `can/src/lib.rs`: `#![warn(missing_docs)]`, `pub mod` every module,
`pub use` the public surface (routing types, options + builder, connector,
gateway, the backend trait, the mock, and the real backend behind its
`cfg`). Cross-reference the BB / REQ ids in the module docs the way the CAN
crate does — those doc links double as traceability.

### Step 11 — Protocol-specific affordances (optional)

If your protocol has operations that don't fit pub/sub (Zenoh's queries),
add them as **inherent methods on the concrete connector type**, *not* on the
`Connector` trait (ADR_0040). See `zenoh/src/connector.rs`'s
`create_querier` / `create_queryable`.

### Step 12 — `crates/taktora-connector-mqtt-tests/Cargo.toml` + tests

The tests crate is an empty-lib crate that exists only to hold the
integration tests and the internal dev-deps:

```toml
[package]
name        = "taktora-connector-mqtt-tests"
version     = "0.1.0"
edition     = { workspace = true }
# ... inherit the rest of the metadata ...
description = "Integration tests for taktora-connector-mqtt. Not published; holds dev-deps on sibling workspace crates so taktora-connector-mqtt's published manifest stays free of internal-crate dev-deps that would race release-plz's topological-sort during publish."
publish     = false

[lib]   # empty stub; the crate exists solely to host tests/ files.

[features]
default = ["rumqttc-integration"]
rumqttc-integration = ["taktora-connector-mqtt/rumqttc-integration"]

[dev-dependencies]
taktora-connector-mqtt          = { workspace = true }
taktora-connector-codec         = { workspace = true }
taktora-connector-core          = { workspace = true }
taktora-connector-host          = { workspace = true }
taktora-connector-transport-iox = { workspace = true }
taktora-executor                = { workspace = true }
serde = { workspace = true, features = ["std", "derive"] }
tokio = { workspace = true, features = ["rt", "rt-multi-thread", "macros", "sync", "time"] }
```

Write, at minimum (mirroring `can-tests/tests/`):

- `trait_surface.rs` — compile-time witness that the type implements
  `Connector` and `name()`/`health()` behave (start in `Connecting`).
- `end_to_end.rs` — a pub/sub round-trip through `MockMqttSession`.

Plus one test per protocol-specific behaviour, each mapped to a `TEST_xxxx`.
Hardware/broker-dependent tests stay gated behind the `*-integration`
feature and/or an env var (CONTRIBUTING.md lists the conventions:
`vcan0`, `ETHERCAT_TEST_NIC`, a running Zenoh router, etc.).

### Step 13 — A runnable example (recommended)

Add `examples/mqtt-pubsub-mock/` modelled on `examples/zenoh-pubsub-mock/`.
The wiring an app does is short and worth showing:

```rust
// 1. Construct the connector backed by the mock session.
let opts = MqttConnectorOptions::builder().tokio_worker_threads(1).build();
let state = Arc::new(MqttState::new(opts));
let mut connector = MqttConnector::new(state, MockMqttSession::new(), JsonCodec)?;

// 2. Matching descriptors (create the reader before the writer).
let routing = MqttRouting::new(MqttTopic::new("taktora/examples/pubsub")?, QoS::AtLeastOnce);
let desc = ChannelDescriptor::<MqttRouting, 256>::new("taktora.examples.pubsub", routing)?;
let reader = connector.create_reader::<Tick, 256>(&desc)?;
let writer = connector.create_writer::<Tick, 256>(&desc)?;

// 3. Register with an executor (or a ConnectorHost) and run.
let mut exec = Executor::builder().worker_threads(1).build()?;
connector.register_with(&mut exec)?;
// add publisher/subscriber/health items, then exec.run()?;
```

Apps that host several connectors use `ConnectorHost` instead of a bare
`Executor`:

```rust
let mut host = ConnectorHost::builder().worker_threads(2).build()?;
let connector = host.register(connector)?; // calls register_with for you
host.run()?;                                // or run_for / run_n
```

---

## 4. Build, test, lint, commit

Run exactly what CI enforces (CONTRIBUTING.md §"Building, testing, linting"):

```bash
cargo build --workspace
cargo test --workspace --all-features -- --test-threads=1   # single-threaded: shared-mem services + process-wide allocator
cargo clippy --workspace --all-targets --all-features -- -D warnings
typos
```

Validate the **committed** spec tree builds with warnings-as-errors (the spec
gate checks disk, including untracked files — never `git add -A` blindly):

```bash
git archive HEAD | tar -x -C "$(mktemp -d)"   # build the archived tree, then:
# (cd that tree) sphinx-build -W -b html spec spec/_build
```

Commit with [Conventional Commits](https://www.conventionalcommits.org/) so
release-plz versions correctly, e.g.:

```
feat(connector-mqtt): add MQTT reference connector (FEAT_00xx)
```

When the implementation lands, flip the relevant `req::` needs to
`:status: implemented` and add their `:links:` to `BB_00xx` + `TEST_00xx`
(required by the sphinx-build `-W` traceability gate).

---

## 5. Checklist

**Spec (before/with the PR)**

- [ ] RFC issue opened (`kind:rfc`, `area:connector-<proto>`).
- [ ] `feat::` + child `req::` drafted in `spec/requirements/connector/<proto>.rst`, `:satisfies: FEAT_0030`, anti-goals listed.
- [ ] `spec::` building blocks (`BB_00x0` + sub-blocks) + an `impl::` directive in `spec/architecture/connector/`.
- [ ] `test::` needs in `spec/verification/connector/<proto>.rst`, each `:verifies:` a REQ.
- [ ] `:status:` promoted `draft → open`.

**Code**

- [ ] Two crates created: `taktora-connector-<proto>` and `…-tests` (`publish = false`).
- [ ] Both added to root `[workspace.members]`; published crate added to `[workspace.dependencies]`.
- [ ] Real backend behind a default-off `*-integration` feature (and `cfg`-gated if platform-specific); mock backend always built.
- [ ] `routing.rs` — typed routing struct, validated identifiers, `impl Routing`.
- [ ] `options.rs` — typed builder with bridge capacities/thresholds.
- [ ] `health.rs` — `HealthMonitor` wrapper with independent subscriptions.
- [ ] Backend trait + `MockXSession` + (gated) `RealXSession`.
- [ ] `gateway.rs` / `bridge.rs` / `registry.rs` / `dispatcher.rs`.
- [ ] `connector.rs` — implements `Connector`; `.out`/`.in` service naming; session moved on first `register_with`; heartbeat item added.
- [ ] `lib.rs` — re-exports + BB/REQ doc cross-refs.
- [ ] Tests crate: `trait_surface.rs`, `end_to_end.rs`, + per-REQ tests; hardware tests gated.
- [ ] (Recommended) `examples/<proto>-pubsub-mock/`.

**Verify**

- [ ] `cargo build/test/clippy/typos` all green at CI's flags.
- [ ] Committed `spec/` builds under `sphinx-build -W`.
- [ ] `req::` flipped to `implemented` with `:links:` to BB + TEST.
- [ ] Conventional-commit message.

---

## 6. Reference: common vs. protocol-specific

| File | Boilerplate (copy & rename) | Protocol-specific (rewrite) |
|------|------------------------------|------------------------------|
| `Cargo.toml` (both crates) | structure, feature/lint shape, the four framework deps | protocol stack dep + feature name |
| `routing.rs` | `impl Routing for X {}` | the struct fields + validation |
| `options.rs` | builder skeleton, clamping | the knobs |
| `health.rs` | monitor wrapper + fan-out | endpoint aggregation strategy |
| `gateway.rs` | tokio runtime ownership + Drop budget | — |
| `bridge.rs` / `registry.rs` | bounded channels + binding table | — |
| `dispatcher.rs` | loop scaffold | the protocol I/O + error→health mapping |
| backend trait + mock/real | the trait/mock pattern | the async ops + real stack |
| `connector.rs` | the `Connector` impl shape, `.out`/`.in`, slot/heartbeat | routing validation, payload-size check |
| tests crate | `publish=false`, dev-deps, `trait_surface.rs` | per-protocol behaviours |

**Canonical files to read first:** `crates/taktora-connector-can/` (whole
crate — the minimal complete connector), then
`crates/taktora-connector-host/src/connector.rs` (the contract) and
`crates/taktora-connector-transport-iox/src/{factory,envelope,channel}.rs`
(the transport you bind to).
