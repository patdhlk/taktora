//! [`J1939Connector`] — plugin-side implementation of the framework's
//! [`Connector`] trait. `BB_0099`, `REQ_0890`, `REQ_0891`.
//!
//! Mirrors `taktora_connector_can::CanConnector` (`REQ_0899`): it owns
//! an iceoryx2 [`Node`], a `Vec<I>` of pre-built drivers (one per
//! configured interface, moved into per-iface dispatcher tasks on
//! `register_with`), a [`J1939Gateway`] holding the tokio runtime, and
//! shared [`J1939State`].
//!
//! On `create_writer` / `create_reader`:
//!
//! 1. Validate the channel's `N` const generic equals the routing's
//!    transport-class max payload (`SingleFrame` → 8, `Tp { max_len }`
//!    → `max_len`), mismatch → [`ConnectorError::Configuration`]
//!    (`REQ_0891`).
//! 2. Open the plugin-side iceoryx2 service `"{name}.out"` / `.in`.
//! 3. Open the paired gateway-side raw port on the same service.
//! 4. Register the channel on the shared [`J1939Registry`].
//!
//! On `register_with`: take the driver vec out (error if called twice)
//! and spawn one [`dispatcher_loop`] per interface; add a heartbeat
//! executor item.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use iceoryx2::node::Node;
use iceoryx2::prelude::{NodeBuilder, ipc};
use taktora_connector_can::{
    BridgedInboundPublish, CanHealthMonitor, CanIface, CanInterfaceLike, ChannelBinding, Direction,
    InboundPublish, IoxOutboundDrain, OutboundDrain,
};
use taktora_connector_core::{ChannelDescriptor, ConnectorError, ConnectorHealth};
use taktora_connector_host::{Connector, HealthSubscription};
use taktora_connector_transport_iox::{
    ChannelReader, ChannelWriter, ServiceFactory, SliceChannelReader, SliceChannelWriter,
};
use taktora_executor::{ControlFlow, Executor, item_with_triggers};

use crate::addr_claim::ClaimGate;
use crate::dispatcher::{DEFAULT_TX_TICK, dispatcher_loop};
use crate::gateway::J1939Gateway;
use crate::options::J1939ConnectorOptions;
use crate::registry::J1939Registry;
use crate::routing::J1939Routing;
use crate::writer::J1939Writer;

/// Connector-internal state shared between [`J1939Connector`] and the
/// per-iface dispatcher tasks.
#[derive(Debug)]
pub struct J1939State {
    options: J1939ConnectorOptions,
    health: Arc<CanHealthMonitor>,
    registry: Arc<Mutex<J1939Registry>>,
    stop: Arc<AtomicBool>,
    /// Per-interface address-claim gate (`BB_0102`, `REQ_0898`). Shared
    /// between each interface's dispatcher (which drives it from the
    /// [`crate::addr_claim::AddrClaimEngine`]) and the application's
    /// [`J1939Writer`] (which reads it to gate outbound transmission). All
    /// gates start in `Claiming`.
    claim_gates: HashMap<CanIface, Arc<ClaimGate>>,
}

impl J1939State {
    /// Construct connector-internal state from configured options.
    #[must_use]
    pub fn new(options: J1939ConnectorOptions) -> Self {
        let capacity = options
            .outbound_capacity()
            .saturating_add(options.inbound_capacity());
        let health = Arc::new(CanHealthMonitor::new(&options.ifaces()));
        let claim_gates = options
            .interfaces()
            .iter()
            .map(|i| (i.iface, Arc::new(ClaimGate::new())))
            .collect();
        Self {
            options,
            health,
            registry: Arc::new(Mutex::new(J1939Registry::with_capacity(capacity))),
            stop: Arc::new(AtomicBool::new(false)),
            claim_gates,
        }
    }

    /// Borrow the per-interface address-claim gate, if `iface` is
    /// configured (`REQ_0898`).
    #[must_use]
    pub fn claim_gate(&self, iface: &CanIface) -> Option<Arc<ClaimGate>> {
        self.claim_gates.get(iface).map(Arc::clone)
    }

    /// Borrow the shared health monitor.
    #[must_use]
    pub fn health(&self) -> &CanHealthMonitor {
        &self.health
    }

    /// Borrow the configured options.
    #[must_use]
    pub const fn options(&self) -> &J1939ConnectorOptions {
        &self.options
    }

    /// Borrow the shared channel registry.
    #[must_use]
    pub const fn registry(&self) -> &Arc<Mutex<J1939Registry>> {
        &self.registry
    }
}

/// Plugin-side J1939 connector.
///
/// Generic over a [`CanInterfaceLike`] driver type (reused from
/// `taktora-connector-can`, `REQ_0899`) and a `PayloadCodec`
/// (`REQ_0211`). The `<I, C>` shape mirrors `CanConnector<I, C>`:
/// `CanInterfaceLike::recv` returns `impl Future` and so the driver is
/// not dyn-compatible — the interface type is a generic parameter, not
/// a boxed trait object.
pub struct J1939Connector<I, C>
where
    I: CanInterfaceLike,
{
    state: Arc<J1939State>,
    codec: C,
    node: Arc<Node<ipc::Service>>,
    gateway: J1939Gateway,
    /// `Some(drivers)` until `register_with` consumes them. Index `i`
    /// is the driver for `options.interfaces()[i]`.
    drivers_slot: Mutex<Option<Vec<I>>>,
    tx_tick: Duration,
}

impl<I, C> J1939Connector<I, C>
where
    I: CanInterfaceLike,
{
    /// Construct a plugin-side connector with pre-built driver
    /// instances. `drivers[i]` must be bound to
    /// `state.options().interfaces()[i].iface`.
    ///
    /// # Errors
    ///
    /// Returns [`ConnectorError::Configuration`] when `drivers.len() !=
    /// interfaces.len()`; [`ConnectorError::Stack`] on iceoryx2 node or
    /// tokio runtime construction failure.
    pub fn new(state: Arc<J1939State>, drivers: Vec<I>, codec: C) -> Result<Self, ConnectorError> {
        if drivers.len() != state.options().interfaces().len() {
            return Err(ConnectorError::Configuration(format!(
                "J1939Connector: drivers.len() {} does not match interfaces.len() {}",
                drivers.len(),
                state.options().interfaces().len()
            )));
        }
        let node = NodeBuilder::new()
            .create::<ipc::Service>()
            .map_err(|e| ConnectorError::stack(NodeError(format!("{e:?}"))))?;
        let gateway = J1939Gateway::new(state.options().clone())
            .map_err(|e| ConnectorError::stack(NodeError(format!("gateway runtime: {e:?}"))))?;
        Ok(Self {
            state,
            codec,
            node: Arc::new(node),
            gateway,
            drivers_slot: Mutex::new(Some(drivers)),
            tx_tick: DEFAULT_TX_TICK,
        })
    }

    /// Override the per-iteration TX drain tick.
    #[must_use]
    pub const fn with_tx_tick(mut self, tick: Duration) -> Self {
        self.tx_tick = tick;
        self
    }

    /// Borrow the shared state.
    #[must_use]
    pub const fn state(&self) -> &Arc<J1939State> {
        &self.state
    }

    /// Signal every dispatcher task to exit. Used in tests for clean
    /// teardown before dropping the connector.
    pub fn stop_dispatchers(&self) {
        self.state.stop.store(true, Ordering::Release);
    }

    fn factory(&self) -> ServiceFactory<'_> {
        ServiceFactory::new(&self.node)
    }

    /// Open the ETP large-payload **slice** writer for `name` (#125,
    /// `ADR_0109` tier 2). ETP payloads are variable-length and cannot
    /// carry a compile-time `N`, so they ride the FEAT_0097 slice channel
    /// rather than the const-`N` [`Self::create_writer`] typed path. The
    /// writer is bound to the connector's `max_etp_bytes` ceiling: a send
    /// above it is refused before loaning, so the data segment never grows
    /// past the bound (`REQ_0903`). The segment starts at
    /// [`crate::options::J1939ConnectorOptions::etp_initial_slice_len`] and
    /// grows by `AllocationStrategy::PowerOfTwo`.
    ///
    /// `routing` must carry [`crate::routing::TransportClass::Etp`]; both
    /// ends agree on the slice service name `name` (no `.in`/`.out` split —
    /// the slice channel is a single bulk service the gateway and plugin
    /// both open).
    ///
    /// # Errors
    ///
    /// [`ConnectorError::Configuration`] when `routing` is not an ETP
    /// class; otherwise any iceoryx2 service/publisher error.
    pub fn create_etp_writer(
        &self,
        name: &str,
        routing: J1939Routing,
    ) -> Result<SliceChannelWriter, ConnectorError> {
        validate_etp_routing(&routing)?;
        let opts = self.state.options();
        self.factory()
            .create_slice_writer(name, opts.etp_initial_slice_len(), opts.max_etp_bytes())
    }

    /// Open the ETP large-payload **slice** reader for `name` (#125,
    /// `ADR_0109` tier 2). Pairs with [`Self::create_etp_writer`] on the
    /// same `name`.
    ///
    /// # Errors
    ///
    /// [`ConnectorError::Configuration`] when `routing` is not an ETP
    /// class; otherwise any iceoryx2 service/subscriber error.
    pub fn create_etp_reader(
        &self,
        name: &str,
        routing: J1939Routing,
    ) -> Result<SliceChannelReader, ConnectorError> {
        validate_etp_routing(&routing)?;
        self.factory().create_slice_reader(name)
    }
}

impl<I, C> J1939Connector<I, C>
where
    I: CanInterfaceLike,
    C: taktora_connector_core::PayloadCodec + Clone + Send + 'static,
{
    /// Open an **address-claim-gated** outbound writer for `iface`
    /// (`BB_0102`, `REQ_0898`). The returned [`J1939Writer`] wraps the
    /// framework [`ChannelWriter`] from [`Self::create_writer`] plus the
    /// per-interface [`ClaimGate`]; its `send` / `send_with_correlation`
    /// return [`ConnectorError::Down`] until the interface's address is
    /// `Claimed`, then delegate to the inner writer (consistent with the
    /// no-durable-buffering anti-goal `REQ_0292`).
    ///
    /// The `Connector` trait's [`Connector::create_writer`] is left intact
    /// (it still returns the shared framework [`ChannelWriter`], which
    /// cannot carry connector-specific claim state); applications that want
    /// the TX gate use this inherent method instead.
    ///
    /// # Errors
    ///
    /// [`ConnectorError::Configuration`] when `iface` is not a configured
    /// interface or the descriptor fails `N` validation; otherwise any
    /// iceoryx2 service/publisher error.
    pub fn create_gated_writer<T, const N: usize>(
        &self,
        descriptor: &ChannelDescriptor<J1939Routing, N>,
        iface: &CanIface,
    ) -> Result<J1939Writer<T, C, N>, ConnectorError>
    where
        T: serde::Serialize,
    {
        let gate = self.state.claim_gate(iface).ok_or_else(|| {
            ConnectorError::Configuration(format!(
                "create_gated_writer: {iface} is not a configured J1939 interface"
            ))
        })?;
        let inner = self.create_writer::<T, N>(descriptor)?;
        Ok(J1939Writer::new(inner, gate))
    }
}

impl<I, C> Connector for J1939Connector<I, C>
where
    I: CanInterfaceLike,
    C: taktora_connector_core::PayloadCodec + Clone + Send + 'static,
{
    type Routing = J1939Routing;
    type Codec = C;

    fn name(&self) -> &str {
        "j1939"
    }

    fn health(&self) -> ConnectorHealth {
        self.state.health.current()
    }

    fn subscribe_health(&self) -> HealthSubscription {
        HealthSubscription::new(self.state.health.subscribe())
    }

    fn register_with(&mut self, executor: &mut Executor) -> Result<(), ConnectorError> {
        let drivers = self
            .drivers_slot
            .lock()
            .expect("drivers slot mutex not poisoned")
            .take()
            .ok_or_else(|| ConnectorError::stack(AlreadyRegistered))?;

        let handle = self
            .gateway
            .handle()
            .ok_or_else(|| ConnectorError::stack(GatewayShutDown))?;

        let interfaces = self.state.options().interfaces().to_vec();
        let tp_timers = self.state.options().tp_timers();
        let max_tp_sessions = self.state.options().max_concurrent_tp_sessions();
        let max_etp_bytes = self.state.options().max_etp_bytes();
        let claim_wait = self.state.options().claim_wait();
        for (interface, driver) in interfaces.into_iter().zip(drivers.into_iter()) {
            let registry = Arc::clone(self.state.registry());
            let health = Arc::clone(&self.state.health);
            let stop = Arc::clone(&self.state.stop);
            let policy = Box::new(taktora_connector_core::ExponentialBackoff::default());
            let tick = self.tx_tick;
            let iface = interface.iface;
            let sa = interface.source_addr;
            let name = interface.name;
            // Reuse the gate the application's J1939Writer reads; default a
            // fresh one if (impossibly) the iface was not pre-registered.
            let claim_gate = self
                .state
                .claim_gate(&iface)
                .unwrap_or_else(|| Arc::new(ClaimGate::new()));
            handle.spawn(async move {
                let _ = dispatcher_loop(
                    iface,
                    sa,
                    name,
                    claim_wait,
                    claim_gate,
                    driver,
                    registry,
                    health,
                    policy,
                    stop,
                    tick,
                    tp_timers,
                    max_tp_sessions,
                    max_etp_bytes,
                )
                .await;
            });
        }

        // Heartbeat ExecutableItem so the connector is a well-formed
        // ConnectorHost participant. Each dispatcher task does the real
        // work; this item satisfies the executor-registration contract.
        let tick = self.tx_tick.max(Duration::from_millis(1));
        let heartbeat = item_with_triggers(
            move |d| {
                d.interval(tick);
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
        let routing = *descriptor.routing();
        validate_routing::<N>(&routing)?;
        let svc_name = service_name(descriptor.name(), Direction::Outbound);
        let plugin_desc = ChannelDescriptor::<J1939Routing, N>::new(svc_name.clone(), routing)?;
        let factory = self.factory();
        let writer = factory.create_writer::<T, _, _, N>(&plugin_desc, self.codec.clone())?;
        let raw_reader = factory.create_raw_reader_named::<N>(&svc_name)?;
        let drain: Box<dyn OutboundDrain> = Box::new(IoxOutboundDrain::<N>::new(raw_reader));
        self.state
            .registry()
            .lock()
            .expect("registry mutex not poisoned")
            .register(
                descriptor.name().to_string(),
                routing,
                Direction::Outbound,
                ChannelBinding::Outbound(drain),
            );
        Ok(writer)
    }

    fn create_reader<T, const N: usize>(
        &self,
        descriptor: &ChannelDescriptor<Self::Routing, N>,
    ) -> Result<ChannelReader<T, Self::Codec, N>, ConnectorError>
    where
        T: serde::de::DeserializeOwned,
    {
        let routing = *descriptor.routing();
        validate_routing::<N>(&routing)?;
        let svc_name = service_name(descriptor.name(), Direction::Inbound);
        let plugin_desc = ChannelDescriptor::<J1939Routing, N>::new(svc_name.clone(), routing)?;
        let factory = self.factory();
        let reader = factory.create_reader::<T, _, _, N>(&plugin_desc, self.codec.clone())?;
        let raw_writer = factory.create_raw_writer_named::<N>(&svc_name)?;
        let inbound_capacity = self.state.options().inbound_capacity();
        let inbound_drop_threshold = self.state.options().inbound_drop_threshold();
        let publish: Box<dyn InboundPublish> = Box::new(BridgedInboundPublish::<N>::new(
            raw_writer,
            inbound_capacity,
            Arc::clone(&self.state.health),
            inbound_drop_threshold,
        ));
        self.state
            .registry()
            .lock()
            .expect("registry mutex not poisoned")
            .register(
                descriptor.name().to_string(),
                routing,
                Direction::Inbound,
                ChannelBinding::Inbound(publish),
            );
        Ok(reader)
    }
}

/// Validate a routing's channel `N` against its transport class
/// (`REQ_0891`). PDU1-vs-PDU2 is irrelevant here — it is derived from
/// the PGN during decode, not declared.
fn validate_routing<const N: usize>(routing: &J1939Routing) -> Result<(), ConnectorError> {
    // ETP rides the variable-length slice channel, not a fixed-`N`
    // envelope; route it through create_etp_writer / create_etp_reader.
    if routing.transport.is_etp() {
        return Err(ConnectorError::Configuration(
            "ETP-class channels do not use the fixed-N create_writer/create_reader path; \
             use create_etp_writer / create_etp_reader (ADR_0109 tier 2)"
                .to_string(),
        ));
    }
    let expected = routing.transport.max_payload();
    if N != expected {
        return Err(ConnectorError::Configuration(format!(
            "ChannelDescriptor max_payload_size {N} does not match TransportClass::{:?}.max_payload() = {expected}",
            routing.transport
        )));
    }
    Ok(())
}

/// Validate an ETP routing for the slice path: it must declare the ETP
/// transport class (#125).
fn validate_etp_routing(routing: &J1939Routing) -> Result<(), ConnectorError> {
    if !routing.transport.is_etp() {
        return Err(ConnectorError::Configuration(format!(
            "create_etp_writer/reader requires TransportClass::Etp, got {:?}",
            routing.transport
        )));
    }
    Ok(())
}

fn service_name(base: &str, direction: Direction) -> String {
    match direction {
        Direction::Outbound => format!("{base}.out"),
        Direction::Inbound => format!("{base}.in"),
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
        write!(
            f,
            "j1939 connector: register_with already called; drivers were moved into dispatcher"
        )
    }
}

impl std::error::Error for AlreadyRegistered {}

#[derive(Debug)]
struct GatewayShutDown;

impl core::fmt::Display for GatewayShutDown {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "j1939 connector: gateway runtime is shut down")
    }
}

impl std::error::Error for GatewayShutDown {}
