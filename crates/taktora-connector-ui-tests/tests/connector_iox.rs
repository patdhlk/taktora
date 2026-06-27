//! Live, in-process iceoryx2 end-to-end test for the assembled [`UiConnector`]
//! (`REQ_0855`, `REQ_0856`, `REQ_0863`, `REQ_0872`, `REQ_0879`).
//!
//! A `UiConnector` is built, declares a ViewModel + a hot scalar + a command,
//! and is registered with an `Executor` — which spawns the pump and
//! command-handler threads. The test then plays the role of a UI client on a
//! **separate** iceoryx2 node in the same process: it reads the manifest, the
//! ViewModel, and the `SystemViewModel` heartbeat off the JSON contract, and
//! round-trips a command (acceptance ack + effect enqueue + `CanExecute`
//! gating).
//!
//! We do not run the executor cyclically: the pump and handler each live on
//! their own OS thread (spawned by `register_with`), so they publish / accept
//! independently of `Executor::run`. Driving a full executor cycle would add
//! nothing the threads do not already do; the threads ARE the connector's work.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use iceoryx2::node::Node;
use iceoryx2::prelude::{NodeBuilder, ipc};
use serde::{Deserialize, Serialize};

use taktora_connector_host::Connector;
use taktora_connector_transport_iox::{RawChannelReader, RawChannelWriter, ServiceFactory};
use taktora_connector_ui::contract::{Ack, Manifest};
use taktora_connector_ui::{CommandParams, ImageEnum, UiConnector, UiConnectorOptions, ViewModel};
use taktora_executor::Executor;

/// Matches `UiConnector::ENVELOPE_CAPACITY` — a UI client opens every service
/// with the connector's fixed envelope capacity.
const N: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ImageEnum)]
#[repr(u8)]
enum StepperState {
    Idle = 0,
    Running = 1,
}

#[derive(Clone, Debug, PartialEq, Serialize, ViewModel)]
struct StepperVm {
    active: bool,
    position: f64,
    state: StepperState,
}

#[derive(Clone, Debug, PartialEq, Deserialize, CommandParams)]
#[command(idempotent)]
struct Enable {
    force: bool,
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique_instance() -> String {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("uiconn_{}_{n}", std::process::id())
}

fn client_node() -> Node<ipc::Service> {
    NodeBuilder::new()
        .create::<ipc::Service>()
        .expect("create client iceoryx2 node")
}

fn corr(byte: u8) -> [u8; 32] {
    let mut id = [0u8; 32];
    id[0] = byte;
    id
}

/// Poll a raw reader briefly for one envelope payload.
fn recv(reader: &RawChannelReader<N>) -> Option<Vec<u8>> {
    let mut dest = [0u8; N];
    for _ in 0..400 {
        if let Ok(Some(sample)) = reader.try_recv_into(&mut dest) {
            return Some(dest[..sample.payload_len].to_vec());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    None
}

#[test]
fn ui_connector_publishes_view_model_manifest_system_and_round_trips_a_command() {
    let instance = unique_instance();

    let options = UiConnectorOptions::builder()
        .instance(instance.clone())
        .epoch(7)
        .publish_cadence(Duration::from_millis(5))
        .command_poll_interval(Duration::from_millis(2))
        .build();

    let mut connector = UiConnector::new(options).expect("create connector");

    // --- Authoring (before register_with) ---
    let stepper = connector.add_view_model::<StepperVm>("Stepper");
    let _rate = connector.add_hot_scalar::<f64>("rate"); // REQ_0863: own service.
    let (effects, can) = connector.add_command::<Enable>("enable");

    // Push a value the pump will publish once a subscriber attaches.
    stepper.set(&StepperVm {
        active: true,
        position: 12.5,
        state: StepperState::Running,
    });

    // --- Registration: spawns the pump + command-handler threads ---
    let mut executor = Executor::builder()
        .worker_threads(0)
        .build()
        .expect("executor");
    connector.register_with(&mut executor).expect("register");

    let node = client_node();
    let factory = ServiceFactory::new(&node);

    // --- Manifest (exempt; published every tick) ---
    let manifest_reader = factory
        .create_raw_reader_named::<N>(&format!("{instance}.manifest"))
        .expect("manifest reader");
    let manifest_bytes = recv(&manifest_reader).expect("manifest delivered");
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes).expect("manifest parses");
    assert_eq!(manifest.instance, instance);
    assert_eq!(manifest.epoch, 7);
    assert!(!manifest.contract_hash.is_empty());
    // The manifest lists the user VM, the hot scalar, the mandatory System VM,
    // and the command.
    let vm_names: Vec<&str> = manifest
        .view_models
        .iter()
        .map(|v| v.name.as_str())
        .collect();
    assert!(vm_names.contains(&"Stepper"), "got {vm_names:?}");
    assert!(
        vm_names.contains(&"rate"),
        "hot scalar must be its own VM entry"
    );
    assert!(
        vm_names.contains(&"System"),
        "System heartbeat must be listed"
    );
    assert_eq!(manifest.commands.len(), 1);
    assert_eq!(manifest.commands[0].name, "enable");
    assert!(manifest.commands[0].idempotent);
    assert!(manifest.commands[0].can_execute_service.is_some());

    // --- ViewModel (non-exempt; force-republished once we attach) ---
    let vm_reader = factory
        .create_raw_reader_named::<N>(&format!("{instance}.vm.Stepper"))
        .expect("vm reader");
    let vm_bytes = recv(&vm_reader).expect("view model delivered");
    let vm_json: serde_json::Value = serde_json::from_slice(&vm_bytes).unwrap();
    assert_eq!(vm_json["position"], 12.5);
    assert_eq!(vm_json["active"], true);
    assert_eq!(vm_json["state"], "Running");

    // --- SystemViewModel heartbeat (exempt; counter advances) ---
    let sys_reader = factory
        .create_raw_reader_named::<N>(&format!("{instance}.vm.System"))
        .expect("system reader");
    let s1 = recv(&sys_reader).expect("system tick 1");
    let s2 = recv(&sys_reader).expect("system tick 2");
    let c1 = serde_json::from_slice::<serde_json::Value>(&s1).unwrap()["counter"]
        .as_u64()
        .unwrap();
    let c2 = serde_json::from_slice::<serde_json::Value>(&s2).unwrap()["counter"]
        .as_u64()
        .unwrap();
    assert!(c2 > c1, "heartbeat counter must advance ({c1} -> {c2})");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&s1).unwrap()["epoch"]
            .as_u64()
            .unwrap(),
        7
    );

    // --- Command round-trip (acceptance ack + effect enqueue) ---
    let req_writer: RawChannelWriter<N> = factory
        .create_raw_writer_named::<N>(&format!("{instance}.cmd.enable.req"))
        .expect("req writer");
    let rep_reader: RawChannelReader<N> = factory
        .create_raw_reader_named::<N>(&format!("{instance}.cmd.enable.rep"))
        .expect("rep reader");

    req_writer
        .send_raw_bytes(b"{\"force\":true}", corr(1))
        .expect("send invocation");

    let ack_bytes = recv(&rep_reader).expect("ack delivered");
    let ack: Ack = serde_json::from_slice(&ack_bytes).expect("ack parses");
    assert_eq!(ack, Ack::Accepted);

    // The effect was enqueued to the application (off the RT path).
    let drained = drain_one(&effects);
    assert_eq!(drained, Some(Enable { force: true }));

    // --- CanExecute gating (REQ_0866): disabling rejects a fresh invocation ---
    can.set(false);
    req_writer
        .send_raw_bytes(b"{\"force\":false}", corr(2))
        .expect("send gated invocation");
    let gated_bytes = recv(&rep_reader).expect("gated ack delivered");
    let gated: Ack = serde_json::from_slice(&gated_bytes).expect("gated ack parses");
    match gated {
        Ack::Rejected { code, .. } => assert_eq!(
            code,
            taktora_connector_ui::contract::RejectedCode::CanExecuteFalse
        ),
        other => panic!("expected CanExecuteFalse rejection, got {other:?}"),
    }
    assert!(
        drain_one(&effects).is_none(),
        "a gated command must not enqueue an effect"
    );

    // Clean shutdown joins the pump + handler threads.
    connector.shutdown();
}

/// Drain one effect, polling briefly (the handler thread enqueues asynchronously).
fn drain_one(rx: &crossbeam_channel::Receiver<Enable>) -> Option<Enable> {
    for _ in 0..200 {
        if let Ok(v) = rx.try_recv() {
            return Some(v);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    None
}
