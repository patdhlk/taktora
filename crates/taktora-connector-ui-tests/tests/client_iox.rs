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

use iceoryx2::node::Node;
use iceoryx2::prelude::{NodeBuilder, ipc};
use serde::{Deserialize, Serialize};

use taktora_connector_host::Connector;
use taktora_connector_transport_iox::{RawChannelReader, ServiceFactory};
use taktora_connector_ui::contract::Ack;
use taktora_connector_ui::{
    CanExecute, CommandParams, ImageEnum, Property, UiConnector, UiConnectorOptions, ViewModel,
};
use taktora_connector_ui_client::{
    BindMode, Client, ClientConfig, ClientError, CommandOutcome, ENVELOPE_CAPACITY, RetryPolicy,
    discover, manifest_service_name,
};
use taktora_executor::Executor;

use serde_json::json;

/// The envelope payload capacity every UI service uses; the raw handles a test
/// opens against a live connector must match the server's capacity.
const CAP: usize = ENVELOPE_CAPACITY;

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

/// A NON-idempotent command (no `#[command(idempotent)]`): unsafe to resend
/// across a restart, so an in-flight epoch change must surface OutcomeUnknown.
#[derive(Clone, Debug, PartialEq, Deserialize, CommandParams)]
struct JogRelative {
    delta: f64,
}

/// A second ViewModel used only to give a restarted connector a *different*
/// structural contract (and therefore a different contract hash).
#[derive(Clone, Debug, PartialEq, Serialize, ViewModel)]
struct ExtraVm {
    counter: u32,
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

/// A live connector exposing a `Stepper` ViewModel and the NON-idempotent
/// `jog_relative` command. The returned tuple keeps every handle alive (and
/// drops the connector before the executor, so the pump/handler threads join
/// cleanly) until the test drops it.
#[allow(clippy::type_complexity)]
fn start_jog_server(
    instance: &str,
    epoch: u64,
) -> (
    UiConnector,
    Executor,
    crossbeam_channel::Receiver<JogRelative>,
    CanExecute,
) {
    let options = UiConnectorOptions::builder()
        .instance(instance.to_owned())
        .epoch(epoch)
        .publish_cadence(Duration::from_millis(5))
        .command_poll_interval(Duration::from_millis(2))
        .build();
    let mut connector = UiConnector::new(options).expect("create connector");
    let stepper = connector.add_view_model::<StepperVm>("Stepper");
    stepper.set(&initial_vm());
    let (effects, can) = connector.add_command::<JogRelative>("jog_relative");
    let mut executor = Executor::builder()
        .worker_threads(0)
        .build()
        .expect("executor");
    connector.register_with(&mut executor).expect("register");
    (connector, executor, effects, can)
}

/// A live connector under `instance` with a *different* contract from
/// [`start_jog_server`] (an extra ViewModel and no command) — and therefore a
/// different contract hash. Used to model an incompatible restart.
fn start_other_server(instance: &str, epoch: u64) -> (UiConnector, Executor) {
    let options = UiConnectorOptions::builder()
        .instance(instance.to_owned())
        .epoch(epoch)
        .publish_cadence(Duration::from_millis(5))
        .command_poll_interval(Duration::from_millis(2))
        .build();
    let mut connector = UiConnector::new(options).expect("create connector");
    let stepper = connector.add_view_model::<StepperVm>("Stepper");
    stepper.set(&initial_vm());
    let _extra = connector.add_view_model::<ExtraVm>("Extra");
    let mut executor = Executor::builder()
        .worker_threads(0)
        .build()
        .expect("executor");
    connector.register_with(&mut executor).expect("register");
    (connector, executor)
}

fn make_node() -> Node<ipc::Service> {
    NodeBuilder::new()
        .create::<ipc::Service>()
        .expect("create iceoryx2 node")
}

/// A fixed 32-byte correlation id seeded by `byte` (the rest zero).
fn corr(byte: u8) -> [u8; 32] {
    let mut id = [0u8; 32];
    id[0] = byte;
    id
}

/// A client config with a short command timeout so restart/epoch tests don't
/// wait the 1s default per attempt.
fn fast_cfg() -> ClientConfig {
    ClientConfig {
        manifest_read_timeout: Duration::from_secs(2),
        command: RetryPolicy {
            timeout: Duration::from_millis(150),
            max_attempts: 5,
        },
    }
}

/// Drain one ack envelope off a reply reader, polling briefly for delivery.
fn recv_ack(reader: &RawChannelReader<CAP>) -> Option<Ack> {
    let mut dest = [0u8; CAP];
    for _ in 0..200 {
        if let Ok(Some(sample)) = reader.try_recv_into(&mut dest) {
            return serde_json::from_slice(&dest[..sample.payload_len]).ok();
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    None
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

#[test]
fn dedupe_replays_cached_ack_without_double_enqueue() {
    // REQ_0867: a retry that reuses the SAME correlation id must replay the
    // server's cached acceptance ack and must NOT enqueue the effect a second
    // time (at-most-once). `Client::invoke` mints a fresh correlation id per
    // call, so to reuse one id across two sends we drive the *live* connector's
    // command plane at the raw transport level, resolving the command's
    // request/reply services from the manifest the client read (REQ_0873).
    let h = start_server(&unique_instance(), 1, initial_vm());
    let hash = live_hash(&h.instance);
    let client = Client::connect(&h.instance, &hash).expect("connect");
    let cmd = client
        .manifest()
        .commands
        .iter()
        .find(|c| c.name == "enable")
        .expect("enable command in manifest")
        .clone();

    let node = make_node();
    let factory = ServiceFactory::new(&node);
    let req = factory
        .create_raw_writer_named::<CAP>(&cmd.request_service)
        .expect("req writer");
    let rep = factory
        .create_raw_reader_named::<CAP>(&cmd.reply_service)
        .expect("rep reader");

    // First send under correlation id `id`.
    let id = corr(0xA1);
    req.send_raw_bytes(b"{\"force\":true}", id).expect("send 1");
    assert_eq!(recv_ack(&rep).expect("ack 1"), Ack::Accepted);

    // Retry: the SAME id again. The server must replay its cached ack.
    req.send_raw_bytes(b"{\"force\":true}", id)
        .expect("send 2 (retry, same id)");
    assert_eq!(
        recv_ack(&rep).expect("ack 2"),
        Ack::Accepted,
        "a same-id retry replays the cached acceptance ack"
    );

    // ...but the effect was enqueued EXACTLY once across the two sends.
    assert_eq!(
        drain_effect(&h.effects),
        Some(Enable { force: true }),
        "the first send enqueues the effect"
    );
    assert!(
        h.effects.try_recv().is_err(),
        "a same-id retry must not enqueue the effect twice (REQ_0867)"
    );
}

#[test]
fn non_idempotent_invoke_across_epoch_is_outcome_unknown() {
    // REQ_0868 / REQ_0882: a non-idempotent command whose epoch changed (a
    // restart) while in flight must surface OutcomeUnknown instead of resending
    // — the restarted server lost the correlation-id dedupe state that made a
    // resend safe.
    //
    // Test seam choice (documented): OutcomeUnknown is only reachable when (a)
    // the invoke times out, (b) the epoch changed, AND (c) the contract hash
    // still matches (so commands stay enabled). In-process a real same-instance
    // restart cannot satisfy all three at once — a matching hash means an
    // identical contract, so a live restarted server would actually answer the
    // command (no timeout), while a server that does not answer necessarily has
    // a different contract (-> read-only, the next test). So we keep the live
    // server only long enough to source a real, hash-matching manifest, then drop
    // it (its command service goes silent) and re-publish that same manifest with
    // a bumped epoch on the real `<instance>.manifest` service. The invoke then
    // exercises the full send -> timeout -> refresh -> epoch-decision path over
    // real iceoryx2.
    let instance = unique_instance();
    let server = start_jog_server(&instance, 1);
    let hash = live_hash(&instance);

    let mut client = Client::connect_with(&instance, &hash, fast_cfg()).expect("connect");
    assert_eq!(client.mode(), BindMode::ReadWrite);
    assert_eq!(client.epoch(), 1);

    // Capture the live, hash-matching manifest, then take the server down.
    let mut manifest = client.manifest().clone();
    drop(server);

    // Re-publish the SAME contract (hash unchanged) with a bumped epoch: this is
    // the "restart" the client observes on its next manifest refresh.
    manifest.epoch = 99;
    let node = make_node();
    let factory = ServiceFactory::new(&node);
    let manifest_writer = factory
        .create_raw_writer_named::<CAP>(&manifest_service_name(&instance))
        .expect("manifest writer");
    let bytes = serde_json::to_vec(&manifest).expect("serialize manifest");
    for _ in 0..5 {
        manifest_writer
            .send_raw_bytes(&bytes, corr(0))
            .expect("publish bumped-epoch manifest");
        std::thread::sleep(Duration::from_millis(5));
    }

    // The send goes unanswered (server is down), the refresh observes the bumped
    // epoch with a still-matching hash, and the non-idempotent-across-epoch rule
    // yields OutcomeUnknown.
    let out = client
        .invoke("jog_relative", &json!({ "delta": 1.0 }))
        .expect("invoke");
    assert_eq!(
        out,
        CommandOutcome::OutcomeUnknown,
        "a non-idempotent command across an epoch change is OutcomeUnknown"
    );
}

#[test]
fn restart_with_incompatible_contract_flips_invoke_to_read_only() {
    // REQ_0876 / REQ_0882: a REAL same-instance restart whose new contract is
    // incompatible (different hash) must flip the client to read-only and abort
    // the in-flight invoke. This is the genuinely reachable real-restart path: to
    // reach the refresh at all the in-flight send must time out, which means the
    // restarted server does not answer this command (a different contract), so
    // the hash necessarily differs and the client goes read-only.
    let instance = unique_instance();
    let server1 = start_jog_server(&instance, 1);
    let hash = live_hash(&instance);

    let mut client = Client::connect_with(&instance, &hash, fast_cfg()).expect("connect");
    assert_eq!(client.mode(), BindMode::ReadWrite);

    // Restart under the SAME instance with a DIFFERENT contract (extra ViewModel,
    // no jog command -> different hash) and a bumped epoch.
    drop(server1);
    let server2 = start_other_server(&instance, 2);

    // The invoke sends (server2 has no jog handler), times out, refreshes onto
    // the incompatible manifest, and aborts read-only.
    let err = client
        .invoke("jog_relative", &json!({ "delta": 1.0 }))
        .expect_err("an incompatible restart must flip the client to read-only");
    assert!(matches!(err, ClientError::ReadOnly), "got {err:?}");
    assert_eq!(client.mode(), BindMode::ReadOnly);

    drop(server2);
}
