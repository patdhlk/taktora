//! [`MqttConnector`] — plugin-side implementation of the framework's
//! [`Connector`] trait (`REQ_0250`).
//!
//! Generic over a [`MqttSessionLike`] back-end `S` (defaulting to the
//! always-built [`crate::MockMqttSession`], so the spec's
//! `MqttConnector<C: PayloadCodec>` spelling is usable verbatim) and a
//! `PayloadCodec` `C` (`REQ_0211`).
//!
//! M2a wires the **outbound** path: `create_writer` opens the plugin-side
//! iceoryx2 publisher and the paired gateway-side raw subscriber, and
//! `register_with` spawns the outbound-drain [`dispatcher_loop`] on the
//! gateway's tokio runtime. `create_reader` opens a valid inbound
//! [`ChannelReader`] and registers its gateway-side publisher, but the
//! subscribe → fan-out delivery that drives it is **M2b** — see the note on
//! [`MqttConnector::create_reader`].

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use iceoryx2::node::Node;
use iceoryx2::prelude::{NodeBuilder, ipc};
use taktora_connector_core::{ChannelDescriptor, ConnectorError, ConnectorHealth};
use taktora_connector_host::{Connector, HealthSubscription};
use taktora_connector_transport_iox::{ChannelReader, ChannelWriter, ServiceFactory};
use taktora_executor::{ControlFlow, Executor, item_with_triggers};

use crate::dispatcher::{
    BridgedInboundPublish, DEFAULT_DISPATCHER_TICK, IoxOutboundDrain, dispatcher_loop,
};
use crate::gateway::MqttGateway;
use crate::health::MqttHealthMonitor;
use crate::inbound::{InboundTable, route_inbound};
use crate::mock::MockMqttSession;
use crate::options::MqttConnectorOptions;
use crate::registry::{ChannelBinding, ChannelDirection, ChannelRegistry, InboundPublish};
use crate::routing::MqttRouting;
use crate::session::{InboundRouter, MqttConnectionState, MqttSessionLike};

/// Tokio worker threads for the gateway runtime. One is enough for the M2a
/// mock path; M3 can raise this when the real `rumqttc` event loop lands.
const GATEWAY_WORKER_THREADS: usize = 1;

/// Connector-internal state shared between [`MqttConnector`] and the
/// gateway-side dispatcher task.
#[derive(Debug)]
pub struct MqttState {
    health: Arc<MqttHealthMonitor>,
    options: MqttConnectorOptions,
    registry: Arc<Mutex<ChannelRegistry>>,
    inbound: Arc<Mutex<InboundTable>>,
    stop: Arc<AtomicBool>,
}

impl MqttState {
    /// Construct connector-internal state from configured options.
    ///
    /// The registry pre-allocates capacity sized to the sum of the bridge
    /// capacities (a sensible upper bound for steady-state channel count).
    #[must_use]
    pub fn new(options: MqttConnectorOptions) -> Self {
        let cap = options
            .outbound_bridge_capacity()
            .saturating_add(options.inbound_bridge_capacity());
        Self {
            health: Arc::new(MqttHealthMonitor::new()),
            options,
            registry: Arc::new(Mutex::new(ChannelRegistry::with_capacity(cap))),
            inbound: Arc::new(Mutex::new(InboundTable::new())),
            stop: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Borrow the shared health monitor.
    #[must_use]
    pub fn health(&self) -> &Arc<MqttHealthMonitor> {
        &self.health
    }

    /// Borrow the configured options.
    #[must_use]
    pub const fn options(&self) -> &MqttConnectorOptions {
        &self.options
    }

    /// Borrow the shared channel registry.
    #[must_use]
    pub const fn registry(&self) -> &Arc<Mutex<ChannelRegistry>> {
        &self.registry
    }

    /// Borrow the shared inbound demux + subscription table (`ADR_0129`).
    #[must_use]
    pub const fn inbound(&self) -> &Arc<Mutex<InboundTable>> {
        &self.inbound
    }

    /// Clone the dispatcher stop signal.
    #[must_use]
    pub fn stop_signal(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.stop)
    }
}

/// Plugin-side MQTT connector (`REQ_0250`).
///
/// Owns an iceoryx2 [`Node`] (shared by plugin-side and gateway-side ports),
/// a [`MqttGateway`] holding the tokio runtime, the shared [`MqttState`],
/// and the session until `register_with` moves it into the dispatcher.
pub struct MqttConnector<C, S = MockMqttSession>
where
    S: MqttSessionLike,
{
    state: Arc<MqttState>,
    codec: C,
    node: Arc<Node<ipc::Service>>,
    gateway: MqttGateway,
    /// `Some(session)` until `register_with` consumes it.
    session_slot: Mutex<Option<Arc<S>>>,
    tick: Duration,
}

impl<C, S> MqttConnector<C, S>
where
    S: MqttSessionLike,
{
    /// Construct a plugin-side connector.
    ///
    /// Opens a fresh iceoryx2 node and a fresh tokio runtime (the gateway).
    /// The `session` is held until [`Connector::register_with`] moves it into
    /// the dispatcher task.
    ///
    /// # Errors
    ///
    /// [`ConnectorError::Stack`] wrapping any iceoryx2 node creation or tokio
    /// runtime construction failure.
    pub fn new(state: Arc<MqttState>, session: Arc<S>, codec: C) -> Result<Self, ConnectorError> {
        let node = NodeBuilder::new()
            .create::<ipc::Service>()
            .map_err(|e| ConnectorError::stack(NodeError(format!("{e:?}"))))?;
        let gateway = MqttGateway::new(GATEWAY_WORKER_THREADS)
            .map_err(|e| ConnectorError::stack(NodeError(format!("gateway runtime: {e:?}"))))?;
        Ok(Self {
            state,
            codec,
            node: Arc::new(node),
            gateway,
            session_slot: Mutex::new(Some(session)),
            tick: DEFAULT_DISPATCHER_TICK,
        })
    }

    /// Override the per-iteration outbound drain tick.
    #[must_use]
    pub const fn with_tick(mut self, tick: Duration) -> Self {
        self.tick = tick;
        self
    }

    /// Borrow the shared state (registry, health, options, stop signal).
    #[must_use]
    pub const fn state(&self) -> &Arc<MqttState> {
        &self.state
    }

    /// Borrow the iceoryx2 node.
    #[must_use]
    pub const fn node(&self) -> &Arc<Node<ipc::Service>> {
        &self.node
    }

    /// Signal the dispatcher loop to exit. Tests use this for clean teardown
    /// before dropping the connector.
    pub fn stop_dispatcher(&self) {
        self.state.stop.store(true, Ordering::Release);
    }

    /// Internal: build an iceoryx2 [`ServiceFactory`] borrowing this
    /// connector's node.
    fn factory(&self) -> ServiceFactory<'_> {
        ServiceFactory::new(&self.node)
    }

    /// Internal: take the session out of its slot (called once, by
    /// `register_with`).
    fn take_session(&self) -> Option<Arc<S>> {
        self.session_slot
            .lock()
            .expect("session slot mutex not poisoned")
            .take()
    }
}

impl<C, S> Connector for MqttConnector<C, S>
where
    C: taktora_connector_core::PayloadCodec + Clone + Send + Sync + 'static,
    S: MqttSessionLike,
{
    type Routing = MqttRouting;
    type Codec = C;

    fn name(&self) -> &'static str {
        "mqtt"
    }

    fn health(&self) -> ConnectorHealth {
        self.state.health.current()
    }

    fn subscribe_health(&self) -> HealthSubscription {
        HealthSubscription::new(self.state.health.subscribe())
    }

    fn register_with(&mut self, executor: &mut Executor) -> Result<(), ConnectorError> {
        // Move the session out — a second call sees `None` and errors.
        let session = self
            .take_session()
            .ok_or_else(|| ConnectorError::stack(AlreadyRegistered))?;
        let handle = self
            .gateway
            .handle()
            .ok_or_else(|| ConnectorError::stack(GatewayShutDown))?;

        // The mock session comes up Connected; reflect that in health so the
        // connector reports `Up` after registration. The real session's
        // health watcher lands in a later milestone.
        if matches!(session.state(), MqttConnectionState::Connected) {
            let _ = self.state.health.transition_to(ConnectorHealth::Up);
        }

        let registry = Arc::clone(self.state.registry());
        let stop = self.state.stop_signal();
        let tick = self.tick;

        // Spawn the health watcher BEFORE the dispatcher consumes `session`
        // (`ADR_0128`). It polls the connection state, maps it onto
        // `ConnectorHealth` (`REQ_0980`–`REQ_0983`), and replays active
        // SUBSCRIBEs on each reconnect CONNACK (`REQ_0985`). Clone the Arcs
        // it needs so the move below can still take ownership of `session`.
        let session_for_health = Arc::clone(&session);
        let health = Arc::clone(self.state.health());
        let inbound = Arc::clone(self.state.inbound());
        let stop_for_health = Arc::clone(&stop);
        let ceiling = self.state.options().reconnect_attempt_ceiling();
        let poll = tick.max(Duration::from_millis(1));
        handle.spawn(async move {
            run_health_watcher(
                session_for_health,
                health,
                inbound,
                stop_for_health,
                ceiling,
                poll,
            )
            .await;
        });

        handle.spawn(async move {
            let _ = dispatcher_loop(registry, session, stop, tick).await;
        });

        // Heartbeat `ExecutableItem` so the connector is a well-formed
        // `ConnectorHost` participant per `REQ_0272`. The dispatcher does the
        // real work; this item satisfies the executor-registration contract.
        let interval = self.tick.max(Duration::from_millis(1));
        let heartbeat = item_with_triggers(
            move |d| {
                d.interval(interval);
                Ok(())
            },
            |_ctx| Ok(ControlFlow::Continue),
        );
        executor.add(heartbeat).map_err(ConnectorError::stack)?;
        Ok(())
    }

    fn create_writer<T, const N: usize>(
        &self,
        descriptor: &ChannelDescriptor<Self::Routing, N>,
    ) -> Result<ChannelWriter<T, Self::Codec, N>, ConnectorError>
    where
        T: serde::Serialize,
    {
        let routing = descriptor.routing().clone();
        let svc_name = format!("{}.out", descriptor.name());
        let plugin_desc =
            ChannelDescriptor::<MqttRouting, N>::new(svc_name.clone(), routing.clone())?;
        let factory = self.factory();

        // Plugin-side publisher (returned to caller).
        let writer = factory.create_writer::<T, _, _, N>(&plugin_desc, self.codec.clone())?;

        // Gateway-side raw subscriber — drained by the dispatcher into
        // `session.publish` each tick. Held behind `Arc` so the async
        // dispatcher can snapshot-clone it out of the registry lock before
        // awaiting on the session.
        let raw_reader = factory.create_raw_reader_named::<N>(&svc_name)?;
        let drain: Arc<dyn crate::registry::OutboundDrain> =
            Arc::new(IoxOutboundDrain::<N>::new(raw_reader));

        self.state
            .registry()
            .lock()
            .expect("registry mutex not poisoned")
            .register(
                descriptor.name().to_string(),
                routing,
                ChannelDirection::Outbound,
                ChannelBinding::Outbound(drain),
            )?;
        Ok(writer)
    }

    /// Open an inbound [`ChannelReader`] (`REQ_0223`, `REQ_0254`).
    ///
    /// M2b wires the full inbound path: the plugin-side iceoryx2 subscriber
    /// and the gateway-side raw publisher (bridged for saturation
    /// accounting, `REQ_0261`); a demux route in the gateway's
    /// reference-counted table so an inbound PUBLISH matching this channel's
    /// filter is fanned out to this reader (`ADR_0129`, `REQ_0987`); a
    /// broker SUBSCRIBE, deduplicated so a filter shared by several channels
    /// hits the broker once (`REQ_0986`); and the gateway-local
    /// [`InboundRouter`] that drives the fan-out.
    ///
    /// The subscription filter is [`MqttRouting::subscription_filter`] — the
    /// routing's explicit `with_filter` value (which may carry `+` / `#`),
    /// else the concrete topic.
    fn create_reader<T, const N: usize>(
        &self,
        descriptor: &ChannelDescriptor<Self::Routing, N>,
    ) -> Result<ChannelReader<T, Self::Codec, N>, ConnectorError>
    where
        T: serde::de::DeserializeOwned,
    {
        let routing = descriptor.routing().clone();
        let filter = routing.subscription_filter();
        let svc_name = format!("{}.in", descriptor.name());
        let plugin_desc =
            ChannelDescriptor::<MqttRouting, N>::new(svc_name.clone(), routing.clone())?;
        let factory = self.factory();

        // Plugin-side subscriber (returned to caller).
        let reader = factory.create_reader::<T, _, _, N>(&plugin_desc, self.codec.clone())?;

        // Gateway-side raw publisher, bridged for inbound saturation
        // accounting (`REQ_0261`). The demux router forwards matched bytes
        // through it onto this channel's inbound iox service.
        let raw_writer = factory.create_raw_writer_named::<N>(&svc_name)?;
        let publisher: Arc<dyn InboundPublish> = Arc::new(BridgedInboundPublish::<N>::new(
            raw_writer,
            self.state.options().inbound_bridge_capacity(),
            Arc::clone(self.state.health()),
            self.state.options().inbound_drop_threshold(),
        ));

        // Demux route + reference-counted broker SUBSCRIBE (dedup) +
        // idempotent router install.
        self.register_inbound(descriptor.name(), &filter, Arc::clone(&publisher))?;

        // Keep the registry Inbound binding for dup-detection + symmetry
        // with the outbound path; it wraps the SAME publisher (no second
        // raw writer).
        self.state
            .registry()
            .lock()
            .expect("registry mutex not poisoned")
            .register(
                descriptor.name().to_string(),
                routing,
                ChannelDirection::Inbound,
                ChannelBinding::Inbound(Box::new(ArcInboundPublish::new(publisher))),
            )?;
        Ok(reader)
    }
}

impl<C, S> MqttConnector<C, S>
where
    C: taktora_connector_core::PayloadCodec + Clone + Send + Sync + 'static,
    S: MqttSessionLike,
{
    /// Register an inbound channel's demux route, reference-count the
    /// broker SUBSCRIBE (dedup per distinct filter, `REQ_0986`), and
    /// ensure the gateway-local demux router is installed on the session.
    fn register_inbound(
        &self,
        name: &str,
        filter: &crate::topic::MqttTopicFilter,
        publisher: Arc<dyn InboundPublish>,
    ) -> Result<(), ConnectorError> {
        let need_subscribe = {
            let mut table = self
                .state
                .inbound()
                .lock()
                .expect("inbound table mutex not poisoned");
            table.add_route(filter.clone(), publisher, name.to_string())
        };
        self.install_inbound_router();
        if need_subscribe {
            let session = self.session_snapshot()?;
            let handle = self
                .gateway
                .handle()
                .ok_or_else(|| ConnectorError::stack(GatewayShutDown))?;
            // `subscribe` is async; the per-filter sink is unused (demux runs
            // through the router), so a no-op sink registers pure interest.
            let sub = handle
                .block_on(session.subscribe(filter, Box::new(|_: &[u8]| {})))
                .map_err(|e| ConnectorError::stack(SessionFailure(format!("{e}"))))?;
            self.state
                .inbound()
                .lock()
                .expect("inbound table mutex not poisoned")
                .record_subscription(filter.clone(), sub);
        }
        Ok(())
    }

    /// Install the gateway-local demux [`InboundRouter`] on the session
    /// (`ADR_0129`). Idempotent — the closure captures the shared inbound
    /// table, so re-installing is a no-op replacement. Best-effort: does
    /// nothing once the session has been moved into the dispatcher.
    fn install_inbound_router(&self) {
        let table = Arc::clone(self.state.inbound());
        let router: InboundRouter = Arc::new(move |topic: &_, payload: &[u8]| {
            route_inbound(&table, topic, payload);
        });
        if let Some(session) = self.peek_session() {
            session.set_inbound_router(router);
        }
    }

    /// Clone the session out of its slot without consuming it. `None` once
    /// `register_with` has moved it into the dispatcher.
    fn peek_session(&self) -> Option<Arc<S>> {
        self.session_slot
            .lock()
            .expect("session slot mutex not poisoned")
            .as_ref()
            .map(Arc::clone)
    }

    /// Snapshot the session, erroring if it has already been consumed by
    /// `register_with`.
    fn session_snapshot(&self) -> Result<Arc<S>, ConnectorError> {
        self.peek_session()
            .ok_or_else(|| ConnectorError::stack(SessionAlreadyTaken))
    }
}

/// Owning wrapper letting the registry hold an inbound publisher behind
/// `Box<dyn InboundPublish>` while the demux table (and, on the real
/// back-end, the session callback) hold clones of the same `Arc`.
struct ArcInboundPublish {
    inner: Arc<dyn InboundPublish>,
}

impl ArcInboundPublish {
    const fn new(inner: Arc<dyn InboundPublish>) -> Self {
        Self { inner }
    }
}

impl InboundPublish for ArcInboundPublish {
    fn publish_bytes(&self, bytes: &[u8]) -> Result<(), ConnectorError> {
        self.inner.publish_bytes(bytes)
    }
}

/// Health watcher task (`ADR_0128`). Polls the session's connection state,
/// maps it onto `ConnectorHealth` (`REQ_0980`–`REQ_0983`), and replays all
/// active SUBSCRIBEs on each reconnect CONNACK (`REQ_0985`). Exits when the
/// stop flag is set or a terminal `Down` is reached.
async fn run_health_watcher<S>(
    session: Arc<S>,
    health: Arc<MqttHealthMonitor>,
    inbound: Arc<Mutex<InboundTable>>,
    stop: Arc<AtomicBool>,
    ceiling: u32,
    poll: Duration,
) where
    S: MqttSessionLike,
{
    let mut last_state = session.state();
    while !stop.load(Ordering::Acquire) {
        tokio::time::sleep(poll).await;
        let state = session.state();
        // A fresh CONNACK (transition into `Connected`) means the clean
        // session dropped all broker-side subscriptions — replay them.
        if is_reconnect(&last_state, &state) {
            replay_subscriptions(&inbound, &session).await;
        }
        let target = map_health(&state, session.reconnect_attempts(), ceiling);
        let terminal = matches!(target, ConnectorHealth::Down { .. });
        health.apply_target(target);
        last_state = state;
        if terminal {
            // `Down` is terminal (auth reject / ceiling exceeded); no
            // further transitions are observable (`REQ_0982`, `REQ_0983`).
            break;
        }
    }
}

/// A reconnect is a transition *into* `Connected` from any other state.
fn is_reconnect(last: &MqttConnectionState, now: &MqttConnectionState) -> bool {
    matches!(now, MqttConnectionState::Connected)
        && !matches!(last, MqttConnectionState::Connected)
}

/// Map a connection-state observation onto a target `ConnectorHealth`
/// (`REQ_0980`–`REQ_0983`). Single-exit.
fn map_health(state: &MqttConnectionState, attempts: u32, ceiling: u32) -> ConnectorHealth {
    match state {
        MqttConnectionState::AuthRejected { reason } => ConnectorHealth::Down {
            reason: format!("authentication rejected: {reason}"),
            since: Instant::now(),
        },
        _ if attempts > ceiling => ConnectorHealth::Down {
            reason: format!("reconnect attempts {attempts} exceeded ceiling {ceiling}"),
            since: Instant::now(),
        },
        MqttConnectionState::Connected => ConnectorHealth::Up,
        MqttConnectionState::Connecting | MqttConnectionState::Disconnected { .. } => {
            ConnectorHealth::Connecting {
                since: Instant::now(),
            }
        }
    }
}

/// Replay every active SUBSCRIBE from the reference-counted table
/// (`REQ_0985`). The replay handles are held for the connector's lifetime
/// so the clean-session re-subscribe does not immediately UNSUBSCRIBE.
async fn replay_subscriptions<S>(inbound: &Arc<Mutex<InboundTable>>, session: &Arc<S>)
where
    S: MqttSessionLike,
{
    let filters = {
        inbound
            .lock()
            .expect("inbound table mutex not poisoned")
            .active_filters()
    };
    for filter in filters {
        if let Ok(handle) = session.subscribe(&filter, Box::new(|_: &[u8]| {})).await {
            inbound
                .lock()
                .expect("inbound table mutex not poisoned")
                .push_replay_handle(handle);
        }
    }
}

#[derive(Debug)]
struct NodeError(String);

impl core::fmt::Display for NodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "iceoryx2 node: {}", self.0)
    }
}

impl std::error::Error for NodeError {}

#[derive(Debug)]
struct AlreadyRegistered;

impl core::fmt::Display for AlreadyRegistered {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(
            "mqtt connector: register_with already called; session was moved into dispatcher",
        )
    }
}

impl std::error::Error for AlreadyRegistered {}

#[derive(Debug)]
struct GatewayShutDown;

impl core::fmt::Display for GatewayShutDown {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("mqtt connector: gateway runtime has shut down")
    }
}

impl std::error::Error for GatewayShutDown {}

#[derive(Debug)]
struct SessionAlreadyTaken;

impl core::fmt::Display for SessionAlreadyTaken {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("mqtt connector: session has already been consumed by register_with")
    }
}

impl std::error::Error for SessionAlreadyTaken {}

#[derive(Debug)]
struct SessionFailure(String);

impl core::fmt::Display for SessionFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "mqtt connector: session operation failed: {}", self.0)
    }
}

impl std::error::Error for SessionFailure {}
