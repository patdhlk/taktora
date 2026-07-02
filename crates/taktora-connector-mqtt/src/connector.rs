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
use std::time::Duration;

use iceoryx2::node::Node;
use iceoryx2::prelude::{NodeBuilder, ipc};
use taktora_connector_core::{ChannelDescriptor, ConnectorError, ConnectorHealth};
use taktora_connector_host::{Connector, HealthSubscription};
use taktora_connector_transport_iox::{ChannelReader, ChannelWriter, ServiceFactory};
use taktora_executor::{ControlFlow, Executor, item_with_triggers};

use crate::dispatcher::{
    DEFAULT_DISPATCHER_TICK, IoxInboundPublish, IoxOutboundDrain, dispatcher_loop,
};
use crate::gateway::MqttGateway;
use crate::health::MqttHealthMonitor;
use crate::mock::MockMqttSession;
use crate::options::MqttConnectorOptions;
use crate::registry::{ChannelBinding, ChannelDirection, ChannelRegistry};
use crate::routing::MqttRouting;
use crate::session::{MqttConnectionState, MqttSessionLike};

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

    /// Open an inbound [`ChannelReader`] (`REQ_0223`).
    ///
    /// M2a creates the plugin-side iceoryx2 subscriber and the paired
    /// gateway-side raw publisher, and registers the inbound binding — so the
    /// service exists and the handle is valid. **Actual inbound delivery
    /// (subscribe → wildcard demux → fan-out to this reader) is M2b**; the
    /// M2a dispatcher never drives the registered [`IoxInboundPublish`]. M2b
    /// picks up the `Inbound` binding registered here.
    fn create_reader<T, const N: usize>(
        &self,
        descriptor: &ChannelDescriptor<Self::Routing, N>,
    ) -> Result<ChannelReader<T, Self::Codec, N>, ConnectorError>
    where
        T: serde::de::DeserializeOwned,
    {
        let routing = descriptor.routing().clone();
        let svc_name = format!("{}.in", descriptor.name());
        let plugin_desc =
            ChannelDescriptor::<MqttRouting, N>::new(svc_name.clone(), routing.clone())?;
        let factory = self.factory();

        // Plugin-side subscriber (returned to caller).
        let reader = factory.create_reader::<T, _, _, N>(&plugin_desc, self.codec.clone())?;

        // Gateway-side raw publisher — M2b's session subscribe callbacks
        // republish MQTT-delivered bytes through it onto this service.
        let raw_writer = factory.create_raw_writer_named::<N>(&svc_name)?;
        let publish: Box<dyn crate::registry::InboundPublish> =
            Box::new(IoxInboundPublish::<N>::new(raw_writer));

        self.state
            .registry()
            .lock()
            .expect("registry mutex not poisoned")
            .register(
                descriptor.name().to_string(),
                routing,
                ChannelDirection::Inbound,
                ChannelBinding::Inbound(publish),
            )?;
        Ok(reader)
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
