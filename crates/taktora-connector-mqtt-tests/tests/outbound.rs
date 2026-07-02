//! Outbound (publish) path end-to-end via `MockMqttSession`.
//! `REQ_0250`, `REQ_0252`, `REQ_0253`, `REQ_0258`.
//!
//! A typed value written through `ChannelWriter::send` is drained by the
//! gateway dispatcher and reaches the session's `publish` with the correct
//! topic, QoS (`REQ_0252`), and retained flag (`REQ_0253`).

use std::sync::Arc;
use std::time::{Duration, Instant};

use taktora_connector_codec::JsonCodec;
use taktora_connector_core::{ChannelDescriptor, ConnectorHealthKind, PayloadCodec};
use taktora_connector_host::Connector;
use taktora_connector_mqtt::{
    MockMqttSession, MqttConnector, MqttConnectorOptions, MqttQos, MqttRouting, MqttState,
    MqttTopic, dispatch_outbound_once,
};
use taktora_executor::Executor;

const N: usize = 256;

fn make_connector(session: &Arc<MockMqttSession>) -> MqttConnector<JsonCodec, MockMqttSession> {
    let state = Arc::new(MqttState::new(MqttConnectorOptions::builder().build()));
    MqttConnector::new(state, Arc::clone(session), JsonCodec).expect("construct MqttConnector")
}

/// Drive the async outbound drain once on a local current-thread runtime,
/// returning the number of successful publishes. The runtime is created and
/// dropped in this (synchronous) call, so the connector's own gateway
/// runtime is never dropped inside an async context.
fn drain_once(
    connector: &MqttConnector<JsonCodec, MockMqttSession>,
    session: &Arc<MockMqttSession>,
) -> usize {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut scratch = vec![0u8; 512];
    rt.block_on(dispatch_outbound_once(
        connector.state().registry(),
        session,
        &mut scratch,
    ))
}

/// Deterministic single-drain drive: no background task — the test steps the
/// dispatcher once and inspects the mock. Verifies topic + QoS + retained +
/// payload reach `session.publish` intact.
#[test]
fn outbound_publish_reaches_session_with_qos_and_retained() {
    let session = Arc::new(MockMqttSession::new());
    let connector = make_connector(&session);

    let routing = MqttRouting::new(
        MqttTopic::new("robot/telemetry").unwrap(),
        MqttQos::AtLeastOnce,
    )
    .with_retained(true);
    let desc =
        ChannelDescriptor::<MqttRouting, N>::new("robot.telemetry".to_string(), routing).unwrap();
    let writer = connector.create_writer::<u32, N>(&desc).unwrap();

    writer.send(&7_u32).expect("send");

    // Step the outbound dispatcher once against the shared session.
    let published = drain_once(&connector, &session);
    assert_eq!(published, 1, "one envelope drained and published");

    let recs = session.published_detailed();
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0].topic, "robot/telemetry");
    assert_eq!(recs[0].qos, MqttQos::AtLeastOnce, "REQ_0252: QoS preserved");
    assert!(recs[0].retained, "REQ_0253: retained flag preserved");
    let decoded: u32 = JsonCodec.decode(&recs[0].payload).expect("decode payload");
    assert_eq!(decoded, 7);
}

/// QoS 0 (`AtMostOnce`) and a cleared retained flag also round-trip
/// (`REQ_0252`).
#[test]
fn outbound_publish_qos0_not_retained() {
    let session = Arc::new(MockMqttSession::new());
    let connector = make_connector(&session);

    let routing = MqttRouting::new(MqttTopic::new("robot/cmd").unwrap(), MqttQos::AtMostOnce);
    let desc = ChannelDescriptor::<MqttRouting, N>::new("robot.cmd".to_string(), routing).unwrap();
    let writer = connector.create_writer::<u32, N>(&desc).unwrap();

    writer.send(&1_u32).unwrap();
    writer.send(&2_u32).unwrap();

    let published = drain_once(&connector, &session);
    assert_eq!(published, 2);

    let recs = session.published_detailed();
    assert_eq!(recs.len(), 2);
    for rec in &recs {
        assert_eq!(rec.topic, "robot/cmd");
        assert_eq!(rec.qos, MqttQos::AtMostOnce);
        assert!(!rec.retained);
    }
}

/// Full lifecycle: `register_with` spawns the outbound-drain dispatcher on
/// the crate-contained tokio runtime (`REQ_0258`), and a subsequent
/// `ChannelWriter::send` reaches the session asynchronously without the test
/// touching any tokio type. Health flips to `Up` after registration.
#[test]
fn register_with_drives_outbound_publish_to_session() {
    let session = Arc::new(MockMqttSession::new());
    let mut connector = make_connector(&session);

    let routing = MqttRouting::new(
        MqttTopic::new("robot/status").unwrap(),
        MqttQos::AtLeastOnce,
    );
    let desc =
        ChannelDescriptor::<MqttRouting, N>::new("robot.status".to_string(), routing).unwrap();
    let writer = connector.create_writer::<u32, N>(&desc).unwrap();

    let mut executor = Executor::builder().worker_threads(0).build().unwrap();
    connector.register_with(&mut executor).unwrap();
    assert_eq!(
        connector.health().kind(),
        ConnectorHealthKind::Up,
        "mock session Connected → connector Up after register_with"
    );

    writer.send(&99_u32).expect("send");

    // Dispatcher ticks at ~1ms; allow up to 2s for the publish to land.
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut recs = session.published_detailed();
    while recs.is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
        recs = session.published_detailed();
    }
    connector.stop_dispatcher();

    assert_eq!(
        recs.len(),
        1,
        "expected the published value to reach session"
    );
    assert_eq!(recs[0].topic, "robot/status");
    assert_eq!(recs[0].qos, MqttQos::AtLeastOnce);
    let decoded: u32 = JsonCodec.decode(&recs[0].payload).expect("decode");
    assert_eq!(decoded, 99);
}
