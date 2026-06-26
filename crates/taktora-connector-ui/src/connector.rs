//! [`UiConnector`]: the MVVM UI connector's [`Connector`] implementation and its
//! authoring API (`REQ_0855`, `REQ_0856`, `REQ_0863`).
//!
//! The connector is assembled in two phases:
//!
//! 1. **Authoring (before [`register_with`](Connector::register_with)).** The
//!    application declares its ViewModels and commands with
//!    [`add_view_model`](UiConnector::add_view_model),
//!    [`add_hot_scalar`](UiConnector::add_hot_scalar) (`REQ_0863`), and
//!    [`add_command`](UiConnector::add_command). Each call hands the application
//!    the RT-side handle it needs — a move-only [`Property`] writer, or a command
//!    effect [`Receiver`] plus a [`CanExecute`] gate — while the connector keeps
//!    the pump-side reader / registration and contributes the schema to the
//!    manifest. Declaring after registration is a configuration bug and panics.
//!
//! 2. **Registration ([`register_with`](Connector::register_with)).** The
//!    connector builds the manifest (`REQ_0872`), assembles the non-RT
//!    [`Pump`] (every ViewModel as a non-exempt entry, the manifest and the
//!    mandatory [`SystemViewModel`] heartbeat as exempt entries, and each
//!    command's [`CanExecute`] as a published bool), wires the
//!    [`IoxCommandTransport`] + [`CommandHandler`], spawns the pump and
//!    command-handler threads (each on its own OS thread, never the executor's
//!    WaitSet thread), and adds a heartbeat `ExecutableItem` so it is a
//!    well-formed `ConnectorHost` participant (mirroring the Zenoh connector).
//!
//! The MVVM helpers are **additive** over the framework's
//! [`create_writer`](Connector::create_writer) /
//! [`create_reader`](Connector::create_reader): those are still honored (they
//! delegate to the iceoryx2 [`ServiceFactory`] with instance-prefixed names) so
//! the trait contract holds, even though UI authors normally reach for the MVVM
//! helpers instead.
//!
//! # Envelope capacity
//!
//! Every connector-created service uses a single fixed envelope capacity
//! [`UiConnector::ENVELOPE_CAPACITY`]. A ViewModel's JSON, a command's
//! params/ack JSON, and the whole manifest JSON must each fit within it; a
//! Phase-4 client opens these services with the same capacity. (A per-ViewModel
//! capacity from `MAX_ENCODED_SIZE` would need associated-const array lengths,
//! which stable Rust cannot use as a const-generic argument — hence one fixed
//! connector-wide capacity.)

use crossbeam_channel::Receiver;
use iceoryx2::node::Node;
use iceoryx2::prelude::{NodeBuilder, ipc};
use serde::Serialize;
use serde::de::DeserializeOwned;

use taktora_connector_codec::JsonCodec;
use taktora_connector_core::{ChannelDescriptor, ConnectorError, ConnectorHealth, PayloadCodec};
use taktora_connector_host::{Connector, HealthSubscription};
use taktora_connector_transport_iox::{ChannelReader, ChannelWriter, ServiceFactory};
use taktora_executor::{ControlFlow, Executor, item_with_triggers};

use taktora_connector_ui_contract::{CommandSchema, ViewModelSchema};

use crate::command::{
    CanExecute, CommandHandler, CommandHandlerHandle, CommandParams, IoxCommandTransport,
    RegisteredCommand, can_execute_entry, command_channel,
};
use crate::health::PublishHealth;
use crate::hot_scalar::{HotScalar, HotScalarValue};
use crate::iox_publisher::IoxVmPublisher;
use crate::manifest::{
    ManifestBuilder, can_execute_service_name, command_reply_service_name,
    command_request_service_name, manifest_entry, manifest_service_name, view_model_service_name,
};
use crate::options::UiConnectorOptions;
use crate::property::{Property, PropertyReader};
use crate::pump::{Pump, PumpEntry, PumpHandle, property_entry};
use crate::routing::UiRouting;
use crate::system::{SYSTEM_VIEW_MODEL_NAME, SystemViewModel, system_entry};
use crate::viewmodel::ViewModel;

/// The fixed envelope payload capacity used for every connector-created
/// service. A free const (not an associated const) because stable Rust forbids a
/// generic `Self::CONST` as a const-generic argument. Re-exported publicly as
/// [`UiConnector::ENVELOPE_CAPACITY`]. Both alias the single source of truth in
/// [`taktora_connector_ui_contract::ENVELOPE_CAPACITY`] so the server and every
/// client open the same iceoryx2 payload type. See the module docs.
const ENVELOPE_CAPACITY: usize = taktora_connector_ui_contract::ENVELOPE_CAPACITY;

/// A deferred "open the iceoryx2 publisher and build the pump entry" step.
///
/// The publisher cannot be opened until [`register_with`](Connector::register_with)
/// has created the node, so authoring stashes a closure that builds the entry
/// from the node + the resolved service name once registration runs.
type EntryBuilder =
    Box<dyn FnOnce(&Node<ipc::Service>, &str) -> Result<PumpEntry, ConnectorError> + Send>;

/// One declared ViewModel awaiting registration.
struct VmRegistration {
    name: String,
    schema: ViewModelSchema,
    /// `V::MAX_ENCODED_SIZE`, captured at authoring time so
    /// [`register_with`](Connector::register_with) can fail fast when a single
    /// ViewModel's worst-case JSON cannot fit [`ENVELOPE_CAPACITY`].
    max_encoded_size: usize,
    build_entry: EntryBuilder,
}

/// One declared command awaiting registration.
struct CommandRegistration {
    name: String,
    schema: CommandSchema,
    registered: RegisteredCommand,
    can: CanExecute,
}

/// The MVVM UI connector (`REQ_0855`).
///
/// Generic over the [`PayloadCodec`] used by the additive
/// [`create_writer`](Connector::create_writer) /
/// [`create_reader`](Connector::create_reader) path; defaults to [`JsonCodec`],
/// which is also the codec the MVVM publish/command planes use on the wire.
pub struct UiConnector<C: PayloadCodec = JsonCodec> {
    options: UiConnectorOptions,
    codec: C,
    node: Node<ipc::Service>,
    health: PublishHealth,
    vm_regs: Vec<VmRegistration>,
    cmd_regs: Vec<CommandRegistration>,
    registered: bool,
    pump_handle: Option<PumpHandle>,
    cmd_handle: Option<CommandHandlerHandle>,
}

impl UiConnector<JsonCodec> {
    /// Construct a UI connector with the default [`JsonCodec`].
    ///
    /// Opens a fresh iceoryx2 node the connector owns for the lifetime of its
    /// publish / command services.
    ///
    /// # Errors
    ///
    /// Returns [`ConnectorError::Stack`] wrapping any iceoryx2 node creation
    /// failure.
    pub fn new(options: UiConnectorOptions) -> Result<Self, ConnectorError> {
        Self::with_codec(options, JsonCodec)
    }
}

impl<C: PayloadCodec> UiConnector<C> {
    /// The fixed envelope payload capacity used for every connector-created
    /// service. A public alias of the single source of truth
    /// [`taktora_connector_ui_contract::ENVELOPE_CAPACITY`] (kept here so
    /// existing `UiConnector::<_>::ENVELOPE_CAPACITY` call sites still resolve).
    /// See the module docs.
    pub const ENVELOPE_CAPACITY: usize = taktora_connector_ui_contract::ENVELOPE_CAPACITY;

    /// Construct a UI connector with an explicit codec for the additive
    /// `create_writer` / `create_reader` path.
    ///
    /// # Errors
    ///
    /// Returns [`ConnectorError::Stack`] wrapping any iceoryx2 node creation
    /// failure.
    pub fn with_codec(options: UiConnectorOptions, codec: C) -> Result<Self, ConnectorError> {
        let node = NodeBuilder::new()
            .create::<ipc::Service>()
            .map_err(|e| ConnectorError::stack(UiError(format!("node: {e:?}"))))?;
        Ok(Self {
            options,
            codec,
            node,
            health: PublishHealth::new(),
            vm_regs: Vec::new(),
            cmd_regs: Vec::new(),
            registered: false,
            pump_handle: None,
            cmd_handle: None,
        })
    }

    /// Borrow the configured options.
    #[must_use]
    pub const fn options(&self) -> &UiConnectorOptions {
        &self.options
    }

    /// The instance namespace prefixing every service name (`REQ_0873`).
    #[must_use]
    pub fn instance(&self) -> &str {
        &self.options.instance
    }

    /// Declare a ViewModel and obtain its move-only RT writer (`REQ_0856`).
    ///
    /// Registers `V` under `name` (a latest-value, history-depth-1 struct on its
    /// own service), contributing `V::schema()` to the manifest. The connector
    /// keeps a [`PropertyReader`] for the pump and returns the sole [`Property`]
    /// writer to the application. Must be called **before**
    /// [`register_with`](Connector::register_with).
    ///
    /// # Panics
    ///
    /// Panics if called after [`register_with`](Connector::register_with).
    #[must_use = "the returned Property is the only writer; dropping it means the ViewModel never updates"]
    pub fn add_view_model<V>(&mut self, name: &str) -> Property<V>
    where
        V: ViewModel + Serialize + Send + 'static,
    {
        self.assert_not_registered("add_view_model");
        let prop = Property::<V>::new();
        let reader: PropertyReader<V> = prop.reader();
        let mut schema = V::schema();
        schema.name = name.to_owned();
        let entry_name = name.to_owned();
        let build_entry: EntryBuilder = Box::new(move |node, service| {
            let publisher = IoxVmPublisher::<ENVELOPE_CAPACITY>::create(node, service)?;
            Ok(property_entry(entry_name, reader, publisher))
        });
        self.vm_regs.push(VmRegistration {
            name: name.to_owned(),
            schema,
            max_encoded_size: V::MAX_ENCODED_SIZE,
            build_entry,
        });
        prop
    }

    /// Promote a single hot scalar onto its **own** service (`REQ_0863`).
    ///
    /// A convenience over [`add_view_model`](Self::add_view_model) for a
    /// [`HotScalar<T>`] — a one-field ViewModel published independently of any
    /// struct ViewModel, so a UI can subscribe to just this fast-changing value.
    /// See [`HotScalar`] for the v1 scope.
    ///
    /// # Panics
    ///
    /// Panics if called after [`register_with`](Connector::register_with).
    #[must_use = "the returned Property is the only writer; dropping it means the ViewModel never updates"]
    pub fn add_hot_scalar<T>(&mut self, name: &str) -> Property<HotScalar<T>>
    where
        T: HotScalarValue,
    {
        self.add_view_model::<HotScalar<T>>(name)
    }

    /// Declare a command and obtain its effect receiver + [`CanExecute`] gate
    /// (`REQ_0865`, `REQ_0866`).
    ///
    /// Registers the command under `name`, contributing its [`CommandSchema`]
    /// (params + idempotent flag) to the manifest and publishing a `CanExecute`
    /// bool property. Returns the [`Receiver`] the application drains its effects
    /// from (off the RT path) and the `CanExecute` handle it flips to enable /
    /// disable the command. The gate starts **enabled**. Must be called
    /// **before** [`register_with`](Connector::register_with).
    ///
    /// # Panics
    ///
    /// Panics if called after [`register_with`](Connector::register_with).
    #[must_use]
    pub fn add_command<P>(&mut self, name: &str) -> (Receiver<P>, CanExecute)
    where
        P: CommandParams + DeserializeOwned + Send + 'static,
    {
        self.assert_not_registered("add_command");
        let can = CanExecute::default();
        let (registered, rx) = command_channel::<P>(&can, self.options.command_channel_capacity);
        // Service names are filled in by the manifest builder at registration;
        // `Some(_)` flags that this command has a CanExecute gate.
        let schema = P::command_schema(
            name.to_owned(),
            String::new(),
            String::new(),
            Some(String::new()),
        );
        self.cmd_regs.push(CommandRegistration {
            name: name.to_owned(),
            schema,
            registered,
            can: can.clone(),
        });
        (rx, can)
    }

    /// Stop the pump and command-handler threads, flushing a final tick / drain.
    ///
    /// Idempotent: a second call is a no-op. Called automatically on drop.
    pub fn shutdown(&mut self) {
        if let Some(handle) = self.pump_handle.take() {
            let _ = handle.stop();
        }
        if let Some(handle) = self.cmd_handle.take() {
            let _ = handle.stop();
        }
    }

    fn assert_not_registered(&self, what: &str) {
        assert!(
            !self.registered,
            "UiConnector::{what} called after register_with: every ViewModel and command must be \
             declared before the connector is registered with the executor",
        );
    }
}

impl<C> Connector for UiConnector<C>
where
    C: PayloadCodec + Clone + Send + 'static,
{
    type Routing = UiRouting;
    type Codec = C;

    fn name(&self) -> &str {
        "ui"
    }

    fn health(&self) -> ConnectorHealth {
        self.health.current()
    }

    fn subscribe_health(&self) -> HealthSubscription {
        HealthSubscription::new(self.health.subscribe())
    }

    fn register_with(&mut self, executor: &mut Executor) -> Result<(), ConnectorError> {
        if self.registered {
            return Err(ConnectorError::stack(AlreadyRegistered));
        }
        self.registered = true;

        let instance = self.options.instance.clone();
        let epoch = self.options.epoch;
        let vm_regs = std::mem::take(&mut self.vm_regs);
        let cmd_regs = std::mem::take(&mut self.cmd_regs);

        // Build the manifest from every declared schema plus the mandatory
        // SystemViewModel (`REQ_0879`), so a UI can discover the heartbeat too.
        let mut manifest_builder = ManifestBuilder::new(instance.clone(), epoch)
            .with_view_model(SystemViewModel::schema());
        for reg in &vm_regs {
            manifest_builder = manifest_builder.with_view_model(reg.schema.clone());
        }
        for reg in &cmd_regs {
            manifest_builder = manifest_builder.with_command(reg.schema.clone());
        }
        let manifest = manifest_builder.build();

        // Fail fast on an oversized envelope (`ENVELOPE_CAPACITY` is fixed for
        // every UI service). The manifest is the sole source of service names, so
        // a manifest that cannot fit the envelope would silently disable the
        // whole UI plane at runtime (the pump only logs + degrades health).
        // Catch it at registration instead, before any service is opened.
        let encoded_manifest = serde_json::to_vec(&manifest).map_err(ConnectorError::stack)?;
        if encoded_manifest.len() > ENVELOPE_CAPACITY {
            return Err(ConnectorError::PayloadOverflow {
                actual: encoded_manifest.len(),
                max: ENVELOPE_CAPACITY,
            });
        }
        // Likewise, a single ViewModel whose worst-case JSON exceeds the envelope
        // can never publish; reject it here rather than at the first pump tick.
        for reg in &vm_regs {
            if reg.max_encoded_size > ENVELOPE_CAPACITY {
                return Err(ConnectorError::PayloadOverflow {
                    actual: reg.max_encoded_size,
                    max: ENVELOPE_CAPACITY,
                });
            }
        }

        // Assemble the pump and command handler under one node borrow, so the
        // borrow is released before we mutate `self` (store the handles) below.
        let (pump, handler) = {
            let factory = ServiceFactory::new(&self.node);
            let mut pump = Pump::new();

            // ViewModels — non-exempt (skipped while no UI is watching).
            for reg in vm_regs {
                let service = view_model_service_name(&instance, &reg.name);
                let entry = (reg.build_entry)(&self.node, &service)?;
                pump.add_entry(entry);
            }

            // Manifest — exempt, published every tick (`REQ_0872`).
            let manifest_pub = IoxVmPublisher::<ENVELOPE_CAPACITY>::create(
                &self.node,
                &manifest_service_name(&instance),
            )?;
            pump.add_entry(manifest_entry(manifest, manifest_pub));

            // SystemViewModel heartbeat — exempt (`REQ_0879`).
            let system_pub = IoxVmPublisher::<ENVELOPE_CAPACITY>::create(
                &self.node,
                &view_model_service_name(&instance, SYSTEM_VIEW_MODEL_NAME),
            )?;
            pump.add_entry(system_entry(epoch, system_pub));

            // Command plane: one request/reply port pair + one CanExecute bool
            // property per command. The `IoxCommandTransport` must be fully
            // assembled before the `CommandHandler` is constructed over it, so
            // collect the registrations and wire the handler after the loop.
            let mut transport = IoxCommandTransport::<ENVELOPE_CAPACITY>::new();
            let mut handler_cmds: Vec<(String, RegisteredCommand)> = Vec::new();
            for reg in cmd_regs {
                let CommandRegistration {
                    name,
                    schema: _,
                    registered,
                    can,
                } = reg;
                let req = command_request_service_name(&instance, &name);
                let rep = command_reply_service_name(&instance, &name);
                let reader = factory.create_raw_reader_named::<ENVELOPE_CAPACITY>(&req)?;
                let writer = factory.create_raw_writer_named::<ENVELOPE_CAPACITY>(&rep)?;
                transport.add_command(name.clone(), reader, writer);

                let can_pub = IoxVmPublisher::<ENVELOPE_CAPACITY>::create(
                    &self.node,
                    &can_execute_service_name(&instance, &name),
                )?;
                pump.add_entry(can_execute_entry(name.clone(), &can, can_pub));
                handler_cmds.push((name, registered));
            }

            let mut handler = CommandHandler::new(transport, self.options.dedupe_capacity);
            for (name, registered) in handler_cmds {
                handler.register(name, registered);
            }
            (pump, handler)
        };

        // Spawn the pump on its own thread, feeding PublishHealth from the
        // per-tick stats (`REQ_0883`): a publish error degrades, an otherwise
        // clean tick keeps / restores `Up`.
        let health = self.health.clone();
        let pump_handle = pump.spawn(self.options.publish_cadence, move |stats| {
            if stats.publish_errors > 0 {
                health.degrade("ui pump publish error");
            } else {
                health.mark_running();
            }
        });
        // The pump is up; mark health `Up` immediately (subscriber absence is
        // never a fault).
        self.health.mark_running();

        // Spawn the command handler on its own thread.
        let cmd_handle = handler.spawn(self.options.command_poll_interval);

        self.pump_handle = Some(pump_handle);
        self.cmd_handle = Some(cmd_handle);

        // Heartbeat `ExecutableItem` so the connector is a well-formed
        // `ConnectorHost` participant (`REQ_0272`). The pump / handler threads do
        // the real work; this item only satisfies the registration contract,
        // mirroring the Zenoh connector.
        let tick = self.options.publish_cadence;
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
        let svc_name = format!("{}.{}", self.options.instance, descriptor.name());
        let desc = ChannelDescriptor::<UiRouting, N>::new(svc_name, descriptor.routing().clone())?;
        let factory = ServiceFactory::new(&self.node);
        factory.create_writer::<T, _, _, N>(&desc, self.codec.clone())
    }

    fn create_reader<T, const N: usize>(
        &self,
        descriptor: &ChannelDescriptor<Self::Routing, N>,
    ) -> Result<ChannelReader<T, Self::Codec, N>, ConnectorError>
    where
        T: serde::de::DeserializeOwned,
    {
        let svc_name = format!("{}.{}", self.options.instance, descriptor.name());
        let desc = ChannelDescriptor::<UiRouting, N>::new(svc_name, descriptor.routing().clone())?;
        let factory = ServiceFactory::new(&self.node);
        factory.create_reader::<T, _, _, N>(&desc, self.codec.clone())
    }
}

impl<C: PayloadCodec> Drop for UiConnector<C> {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// A generic error wrapping an iceoryx2 failure for [`ConnectorError::Stack`].
#[derive(Debug)]
struct UiError(String);

impl core::fmt::Display for UiError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "ui connector: {}", self.0)
    }
}

impl std::error::Error for UiError {}

/// Returned by a second [`register_with`](Connector::register_with) call.
#[derive(Debug)]
struct AlreadyRegistered;

impl core::fmt::Display for AlreadyRegistered {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("ui connector: register_with already called")
    }
}

impl std::error::Error for AlreadyRegistered {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use taktora_connector_ui_contract::Kind;

    #[derive(Clone, Debug, PartialEq, Serialize)]
    struct Scalar {
        v: f64,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct ScalarImage {
        v: f64,
    }

    impl ViewModel for Scalar {
        type Image = ScalarImage;
        const IMAGE_SIZE: usize = core::mem::size_of::<ScalarImage>();
        const MAX_ENCODED_SIZE: usize = 32;
        fn schema() -> ViewModelSchema {
            ViewModelSchema {
                name: "Scalar".into(),
                service: String::new(),
                fields: vec![taktora_connector_ui_contract::FieldSchema {
                    name: "v".into(),
                    ty: taktora_connector_ui_contract::FieldType::F64,
                }],
            }
        }
        fn to_image(&self) -> ScalarImage {
            ScalarImage { v: self.v }
        }
        fn from_image(image: &ScalarImage) -> Self {
            Self { v: image.v }
        }
        fn image_to_json(image: &ScalarImage, buf: &mut Vec<u8>) {
            serde_json::to_writer(buf, &Self::from_image(image)).expect("infallible");
        }
    }

    /// A ViewModel whose worst-case JSON cannot fit [`ENVELOPE_CAPACITY`].
    /// Its `MAX_ENCODED_SIZE` deliberately overflows the fixed envelope so
    /// registration must reject it (the encoded bytes are irrelevant — the
    /// associated const alone drives the fail-fast check).
    #[derive(Clone, Debug, PartialEq, Serialize)]
    struct Big {
        v: f64,
    }

    impl ViewModel for Big {
        type Image = ScalarImage;
        const IMAGE_SIZE: usize = core::mem::size_of::<ScalarImage>();
        const MAX_ENCODED_SIZE: usize = ENVELOPE_CAPACITY + 1;
        fn schema() -> ViewModelSchema {
            ViewModelSchema {
                name: "Big".into(),
                service: String::new(),
                fields: vec![taktora_connector_ui_contract::FieldSchema {
                    name: "v".into(),
                    ty: taktora_connector_ui_contract::FieldType::F64,
                }],
            }
        }
        fn to_image(&self) -> ScalarImage {
            ScalarImage { v: self.v }
        }
        fn from_image(image: &ScalarImage) -> Self {
            Self { v: image.v }
        }
        fn image_to_json(image: &ScalarImage, buf: &mut Vec<u8>) {
            serde_json::to_writer(buf, &Self::from_image(image)).expect("infallible");
        }
    }

    #[derive(Clone, Debug, PartialEq, Deserialize)]
    struct Jog {
        delta: f64,
    }

    impl CommandParams for Jog {
        const IDEMPOTENT: bool = false;
        fn params() -> Vec<taktora_connector_ui_contract::FieldSchema> {
            vec![taktora_connector_ui_contract::FieldSchema {
                name: "delta".into(),
                ty: taktora_connector_ui_contract::FieldType::F64,
            }]
        }
    }

    fn connector() -> UiConnector {
        use std::sync::atomic::{AtomicU64, Ordering};
        // A unique instance per connector keeps concurrently-running tests from
        // colliding on the same iceoryx2 service names (which would exceed the
        // per-service publisher limit once several register at once).
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        UiConnector::new(
            UiConnectorOptions::builder()
                .instance(format!("ui_unit_test_{n}"))
                .build(),
        )
        .expect("create connector")
    }

    #[test]
    fn name_is_ui() {
        assert_eq!(connector().name(), "ui");
    }

    #[test]
    fn add_view_model_records_a_registration_and_returns_a_writer() {
        let mut c = connector();
        let prop = c.add_view_model::<Scalar>("Scalar");
        // The returned writer drives the cell; the connector kept the reader.
        prop.set(&Scalar { v: 1.0 });
        assert_eq!(c.vm_regs.len(), 1);
        assert_eq!(c.vm_regs[0].name, "Scalar");
    }

    #[test]
    fn add_hot_scalar_records_a_single_field_view_model() {
        let mut c = connector();
        let _p = c.add_hot_scalar::<f64>("rate");
        assert_eq!(c.vm_regs.len(), 1);
        assert_eq!(c.vm_regs[0].schema.fields.len(), 1);
        assert_eq!(c.vm_regs[0].schema.fields[0].name, "value");
        assert_eq!(c.vm_regs[0].name, "rate");
    }

    #[test]
    fn add_command_records_a_registration_and_returns_gate_enabled() {
        let mut c = connector();
        let (_rx, can) = c.add_command::<Jog>("jog");
        assert!(can.get(), "CanExecute starts enabled");
        assert_eq!(c.cmd_regs.len(), 1);
        assert_eq!(c.cmd_regs[0].name, "jog");
        assert_eq!(c.cmd_regs[0].schema.kind, Kind::Command);
        assert!(c.cmd_regs[0].schema.can_execute_service.is_some());
    }

    #[test]
    #[should_panic(expected = "add_view_model called after register_with")]
    fn add_view_model_after_register_panics() {
        let mut c = connector();
        let mut executor = Executor::builder()
            .worker_threads(0)
            .build()
            .expect("executor");
        c.register_with(&mut executor).expect("register");
        let _ = c.add_view_model::<Scalar>("late");
    }

    #[test]
    #[should_panic(expected = "add_command called after register_with")]
    fn add_command_after_register_panics() {
        let mut c = connector();
        let mut executor = Executor::builder()
            .worker_threads(0)
            .build()
            .expect("executor");
        c.register_with(&mut executor).expect("register");
        let _ = c.add_command::<Jog>("late");
    }

    #[test]
    fn register_with_oversized_view_model_returns_payload_overflow() {
        // A ViewModel whose MAX_ENCODED_SIZE exceeds the fixed envelope must be
        // rejected at registration (fail fast) rather than silently degrading the
        // whole UI plane at runtime.
        let mut c = connector();
        let _writer = c.add_view_model::<Big>("Big");
        let mut executor = Executor::builder()
            .worker_threads(0)
            .build()
            .expect("executor");
        match c.register_with(&mut executor) {
            Err(ConnectorError::PayloadOverflow { actual, max }) => {
                assert_eq!(actual, Big::MAX_ENCODED_SIZE);
                assert_eq!(max, UiConnector::<JsonCodec>::ENVELOPE_CAPACITY);
            }
            other => panic!("expected PayloadOverflow, got {other:?}"),
        }
    }

    #[test]
    fn second_register_with_errors() {
        let mut c = connector();
        let mut executor = Executor::builder()
            .worker_threads(0)
            .build()
            .expect("executor");
        c.register_with(&mut executor).expect("first register");
        assert!(c.register_with(&mut executor).is_err());
    }
}
