//! Live, in-process iceoryx2 end-to-end tests for the reference UI **client**
//! (`taktora-connector-ui-client`) against a real [`UiConnector`] server
//! (`REQ_0864`, `REQ_0865`, `REQ_0876`, `REQ_0877`, `REQ_0880`, `REQ_0881`,
//! `REQ_0882`).
//!
//! Each test stands up a `UiConnector` (which spawns the pump + command-handler
//! threads on `register_with`) under a unique instance namespace, then drives a
//! `Client` over the JSON contract on a separate iceoryx2 node — exercising
//! discovery, hash validation + read-only fallback, per-field property diffing
//! with staleness, command acceptance acks with CanExecute gating, and stateless
//! restart recovery.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use taktora_connector_host::Connector;
use taktora_connector_ui::{
    CanExecute, CommandParams, ImageEnum, Property, UiConnector, UiConnectorOptions, ViewModel,
};
use taktora_connector_ui_client::{BindMode, Client, ClientError, CommandOutcome, discover};
use taktora_executor::Executor;

use serde_json::json;

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
    format!("uiclient_{}_{n}", std::process::id())
}

/// A running server plus the application-side handles a test pokes.
struct Harness {
    // Held only for its `Drop` (which joins the pump + handler threads); field
    // order matters so the connector drops before the executor.
    _connector: UiConnector,
    _executor: Executor,
    stepper: Property<StepperVm>,
    effects: crossbeam_channel::Receiver<Enable>,
    can: CanExecute,
    instance: String,
}

/// Start a connector under `instance`/`epoch` with a `Stepper` ViewModel and an
/// idempotent `enable` command, seeding the ViewModel with `initial`.
fn start_server(instance: &str, epoch: u64, initial: StepperVm) -> Harness {
    let options = UiConnectorOptions::builder()
        .instance(instance.to_owned())
        .epoch(epoch)
        .publish_cadence(Duration::from_millis(5))
        .command_poll_interval(Duration::from_millis(2))
        .build();

    let mut connector = UiConnector::new(options).expect("create connector");
    let stepper = connector.add_view_model::<StepperVm>("Stepper");
    let (effects, can) = connector.add_command::<Enable>("enable");
    stepper.set(&initial);

    let mut executor = Executor::builder()
        .worker_threads(0)
        .build()
        .expect("executor");
    connector.register_with(&mut executor).expect("register");

    Harness {
        _connector: connector,
        _executor: executor,
        stepper,
        effects,
        can,
        instance: instance.to_owned(),
    }
}

fn initial_vm() -> StepperVm {
    StepperVm {
        active: true,
        position: 12.5,
        state: StepperState::Running,
    }
}

/// Read the server's live contract hash (the hash a correctly-generated client
/// would have been built against).
fn live_hash(instance: &str) -> String {
    // Connecting with a placeholder hash still reads the manifest; we only need
    // its contract_hash.
    let client = Client::connect(instance, "placeholder").expect("connect to read hash");
    client.manifest().contract_hash.clone()
}

/// Poll a ViewModel until it reports changes, or panic after a bound.
fn poll_changes(client: &mut Client, vm: &str) -> Vec<taktora_connector_ui_client::PropertyChange> {
    for _ in 0..400 {
        let changes = client.poll_view_model(vm).expect("poll");
        if !changes.is_empty() {
            return changes;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("no property changes within timeout for '{vm}'");
}

/// Drain one effect, polling briefly (the handler enqueues asynchronously).
fn drain_effect(rx: &crossbeam_channel::Receiver<Enable>) -> Option<Enable> {
    for _ in 0..200 {
        if let Ok(v) = rx.try_recv() {
            return Some(v);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    None
}

#[test]
fn discover_lists_the_running_instance_and_connect_binds_it() {
    // REQ_0877 / REQ_0872: registry scan finds the manifest; connect reads it.
    let h = start_server(&unique_instance(), 7, initial_vm());

    let manifests = discover().expect("discover");
    assert!(
        manifests.iter().any(|m| m.instance == h.instance),
        "discover() must list the running instance '{}'; got {:?}",
        h.instance,
        manifests.iter().map(|m| &m.instance).collect::<Vec<_>>()
    );

    let hash = live_hash(&h.instance);
    let client = Client::connect(&h.instance, &hash).expect("connect");
    assert_eq!(client.manifest().instance, h.instance);
    assert_eq!(client.epoch(), 7);
    // The manifest is the sole source of service names (REQ_0873).
    assert!(
        client
            .manifest()
            .view_models
            .iter()
            .any(|v| v.name == "Stepper")
    );
    assert!(
        client
            .manifest()
            .commands
            .iter()
            .any(|c| c.name == "enable")
    );
}

#[test]
fn matching_hash_is_read_write_mismatch_is_read_only_with_commands_disabled() {
    // REQ_0876: hash match -> read-write; mismatch -> read-only, commands off.
    let h = start_server(&unique_instance(), 1, initial_vm());
    let good = live_hash(&h.instance);

    let rw = Client::connect(&h.instance, &good).expect("connect rw");
    assert_eq!(rw.mode(), BindMode::ReadWrite);

    let mut ro = Client::connect(&h.instance, "0000deadbeef").expect("connect ro");
    assert_eq!(ro.mode(), BindMode::ReadOnly);

    let err = ro
        .invoke("enable", &json!({ "force": true }))
        .expect_err("commands must be disabled in read-only mode");
    assert!(matches!(err, ClientError::ReadOnly), "got {err:?}");
}

#[test]
fn property_subscribe_diffs_only_changed_fields_and_tracks_staleness() {
    // REQ_0864 / REQ_0880.
    let h = start_server(&unique_instance(), 1, initial_vm());
    let hash = live_hash(&h.instance);
    let mut client = Client::connect(&h.instance, &hash).expect("connect");
    client.subscribe("Stepper").expect("subscribe");

    // First value: every field is reported as changed.
    let first = poll_changes(&mut client, "Stepper");
    let first_fields: Vec<&str> = first.iter().map(|c| c.field.as_str()).collect();
    assert!(first_fields.contains(&"position"));
    assert!(first_fields.contains(&"active"));
    assert!(first_fields.contains(&"state"));

    // Freshly received -> not stale.
    assert!(
        !client
            .view_model_staleness("Stepper", Duration::from_secs(5))
            .is_stale(),
        "a just-received ViewModel must be fresh"
    );

    // Change only `position`: the diff must report exactly that one field.
    h.stepper.set(&StepperVm {
        active: true,
        position: 99.0,
        state: StepperState::Running,
    });
    let second = poll_changes(&mut client, "Stepper");
    assert_eq!(
        second.len(),
        1,
        "only the changed field raises PropertyChanged, got {second:?}"
    );
    assert_eq!(second[0].field, "position");
    assert_eq!(second[0].value, json!(99.0));
}

#[test]
fn invoke_is_accepted_then_can_execute_false_is_rejected() {
    // REQ_0865 / REQ_0866 / REQ_0867.
    let h = start_server(&unique_instance(), 1, initial_vm());
    let hash = live_hash(&h.instance);
    let mut client = Client::connect(&h.instance, &hash).expect("connect");

    let out = client
        .invoke("enable", &json!({ "force": true }))
        .expect("invoke");
    assert_eq!(out, CommandOutcome::Accepted);
    assert_eq!(
        drain_effect(&h.effects),
        Some(Enable { force: true }),
        "accepted command enqueues its effect"
    );

    // Gate the command off: a fresh invocation is rejected, no effect enqueued.
    h.can.set(false);
    let out = client
        .invoke("enable", &json!({ "force": false }))
        .expect("invoke gated");
    match out {
        CommandOutcome::Rejected { code, .. } => assert_eq!(
            code,
            taktora_connector_ui_client::contract::RejectedCode::CanExecuteFalse
        ),
        other => panic!("expected CanExecuteFalse rejection, got {other:?}"),
    }
    assert!(
        drain_effect(&h.effects).is_none(),
        "a gated command must not enqueue an effect"
    );
}

#[test]
fn fresh_client_recovers_state_statelessly() {
    // REQ_0881: a brand-new client with no prior state recovers the current
    // manifest + ViewModel value purely from history-depth-1 redelivery, with no
    // handshake (the server is not told a client appeared beyond the normal
    // subscriber attach).
    let value = StepperVm {
        active: false,
        position: 42.0,
        state: StepperState::Idle,
    };
    let h = start_server(&unique_instance(), 5, value.clone());
    let hash = live_hash(&h.instance);

    // A FRESH client: connect (reads manifest), subscribe, poll.
    let mut fresh = Client::connect(&h.instance, &hash).expect("fresh connect");
    assert_eq!(fresh.epoch(), 5);
    fresh.subscribe("Stepper").expect("subscribe");

    let changes = poll_changes(&mut fresh, "Stepper");
    let position = changes
        .iter()
        .find(|c| c.field == "position")
        .map(|c| c.value.clone());
    assert_eq!(
        position,
        Some(json!(42.0)),
        "fresh client must recover the current value from depth-1 redelivery"
    );
    assert!(
        fresh.view_model_fields("Stepper").is_some(),
        "fresh client holds the recovered ViewModel"
    );
}
