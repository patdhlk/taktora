//! Reconnection, health mapping, and SUBSCRIBE replay via
//! `MockMqttSession` (`REQ_0980`–`REQ_0985`, `ADR_0128`, `ADR_0130`).
//!
//! The health watcher spawned by `register_with` maps the session's
//! connection state onto `ConnectorHealth`, drives a terminal `Down` on an
//! auth-rejected CONNACK or a breached reconnect ceiling, and replays the
//! reference-counted subscription table on every reconnect CONNACK.

use std::sync::Arc;
use std::time::{Duration, Instant};

use taktora_connector_codec::JsonCodec;
use taktora_connector_core::{ChannelDescriptor, ConnectorHealthKind, PayloadCodec};
use taktora_connector_host::Connector;
use taktora_connector_mqtt::{
    MockMqttSession, MqttConnector, MqttConnectorOptions, MqttQos, MqttRouting, MqttSessionLike,
    MqttState, MqttTopic, MqttTopicFilter,
};
use taktora_executor::Executor;

const N: usize = 256;

fn connector_with(
    session: &Arc<MockMqttSession>,
    options: MqttConnectorOptions,
) -> MqttConnector<JsonCodec, MockMqttSession> {
    let state = Arc::new(MqttState::new(options));
    MqttConnector::new(state, Arc::clone(session), JsonCodec).expect("construct MqttConnector")
}

fn reader_routing(filter: &str) -> MqttRouting {
    MqttRouting::new(MqttTopic::new("placeholder/topic").unwrap(), MqttQos::AtLeastOnce)
        .with_filter(MqttTopicFilter::new(filter).unwrap())
}

fn encode(value: u32) -> Vec<u8> {
    let mut buf = vec![0u8; 64];
    let n = JsonCodec.encode(&value, &mut buf).unwrap();
    buf.truncate(n);
    buf
}

/// Spin until the connector reports `want`, or panic after `timeout`.
fn wait_for_health(
    connector: &MqttConnector<JsonCodec, MockMqttSession>,
    want: ConnectorHealthKind,
    timeout: Duration,
) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if connector.health().kind() == want {
            return;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!(
        "health did not reach {want:?} in time (last = {:?})",
        connector.health().kind()
    );
}

/// `REQ_0980`: the connection state maps onto `ConnectorHealth` — fresh is
/// `Connecting`, a CONNACK is `Up`, and a transient disconnect returns to
/// `Connecting`.
#[test]
fn connection_state_maps_to_health() {
    let session = Arc::new(MockMqttSession::new());
    let mut connector = connector_with(&session, MqttConnectorOptions::builder().build());

    assert_eq!(
        connector.health().kind(),
        ConnectorHealthKind::Connecting,
        "fresh connector is Connecting"
    );

    let mut executor = Executor::builder().worker_threads(0).build().unwrap();
    connector.register_with(&mut executor).unwrap();
    wait_for_health(&connector, ConnectorHealthKind::Up, Duration::from_secs(2));

    // Transient disconnect → Connecting (bridged from Up via ARCH_0012).
    session.simulate_disconnect("broker dropped us");
    wait_for_health(&connector, ConnectorHealthKind::Connecting, Duration::from_secs(2));

    // Reconnect CONNACK → Up again.
    session.simulate_connack();
    wait_for_health(&connector, ConnectorHealthKind::Up, Duration::from_secs(2));

    connector.stop_dispatcher();
}

/// `REQ_0985` (`ADR_0130`): on each reconnect CONNACK the gateway replays
/// every active SUBSCRIBE from its table, and inbound delivery resumes.
#[test]
fn reconnect_replays_active_subscriptions() {
    let session = Arc::new(MockMqttSession::new());
    let mut connector = connector_with(&session, MqttConnectorOptions::builder().build());

    let desc = ChannelDescriptor::<MqttRouting, N>::new(
        "replay.chan".to_string(),
        reader_routing("robot/+/telemetry"),
    )
    .unwrap();
    let reader = connector.create_reader::<u32, N>(&desc).unwrap();
    assert_eq!(
        session.subscribe_calls(),
        vec!["robot/+/telemetry".to_string()],
        "initial SUBSCRIBE at create_reader"
    );

    let mut executor = Executor::builder().worker_threads(0).build().unwrap();
    connector.register_with(&mut executor).unwrap();
    wait_for_health(&connector, ConnectorHealthKind::Up, Duration::from_secs(2));

    // Drop then re-establish the connection.
    session.simulate_disconnect("net blip");
    wait_for_health(&connector, ConnectorHealthKind::Connecting, Duration::from_secs(2));
    session.simulate_connack();
    wait_for_health(&connector, ConnectorHealthKind::Up, Duration::from_secs(2));

    // The clean session forgot its subscriptions; the watcher replayed them.
    let deadline = Instant::now() + Duration::from_secs(2);
    while session.subscribe_calls().len() < 2 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(
        session.subscribe_calls(),
        vec!["robot/+/telemetry".to_string(), "robot/+/telemetry".to_string()],
        "REQ_0985: SUBSCRIBE replayed on reconnect CONNACK"
    );

    // Inbound delivery works after the reconnect.
    session.deliver_inbound(&MqttTopic::new("robot/arm/telemetry").unwrap(), &encode(9));
    let deadline = Instant::now() + Duration::from_millis(500);
    let mut got = None;
    while got.is_none() && Instant::now() < deadline {
        if let Ok(Some(env)) = reader.try_recv() {
            got = Some(env.value);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(got, Some(9), "inbound resumes after reconnect");

    connector.stop_dispatcher();
}

/// `REQ_0982`: an authentication-rejected CONNACK transitions to a terminal
/// `Down`.
#[test]
fn auth_rejected_connack_transitions_to_down() {
    let session = Arc::new(MockMqttSession::new());
    let mut connector = connector_with(&session, MqttConnectorOptions::builder().build());

    let mut executor = Executor::builder().worker_threads(0).build().unwrap();
    connector.register_with(&mut executor).unwrap();
    wait_for_health(&connector, ConnectorHealthKind::Up, Duration::from_secs(2));

    session.simulate_auth_reject("bad credentials");
    wait_for_health(&connector, ConnectorHealthKind::Down, Duration::from_secs(2));

    connector.stop_dispatcher();
}

/// `REQ_0983`: exceeding the configured consecutive-reconnect ceiling
/// transitions to `Down`.
#[test]
fn reconnect_ceiling_exceeded_transitions_to_down() {
    let session = Arc::new(MockMqttSession::new());
    let options = MqttConnectorOptions::builder()
        .reconnect_attempt_ceiling(2)
        .build();
    let mut connector = connector_with(&session, options);

    let mut executor = Executor::builder().worker_threads(0).build().unwrap();
    connector.register_with(&mut executor).unwrap();
    wait_for_health(&connector, ConnectorHealthKind::Up, Duration::from_secs(2));

    // Three consecutive failed reconnects exceed the ceiling of 2.
    session.simulate_failed_reconnect();
    session.simulate_failed_reconnect();
    session.simulate_failed_reconnect();
    assert_eq!(session.reconnect_attempts(), 3);

    wait_for_health(&connector, ConnectorHealthKind::Down, Duration::from_secs(2));

    connector.stop_dispatcher();
}
