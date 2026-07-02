//! Round-trip integration test for the M1 MQTT connector core: a publish
//! on a concrete topic is delivered to a matching (including wildcard)
//! subscription via the in-process `MockMqttSession`, with the payload
//! carried through `JsonCodec` as the default codec (`REQ_0988`).

use std::sync::{Arc, Mutex};

use taktora_connector_codec::JsonCodec;
use taktora_connector_core::PayloadCodec;
use taktora_connector_mqtt::mock::MockMqttSession;
use taktora_connector_mqtt::session::{MqttConnectionState, MqttSessionLike};
use taktora_connector_mqtt::{MqttQos, MqttRouting, MqttTopic, MqttTopicFilter};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Tick {
    seq: u64,
    label: String,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn publish_reaches_matching_wildcard_subscription() {
    let session = MockMqttSession::new();
    assert_eq!(session.state(), MqttConnectionState::Connected);

    // A wildcard subscription that should match the publish topic.
    let received: Arc<Mutex<Vec<Tick>>> = Arc::new(Mutex::new(Vec::new()));
    let sink_received = Arc::clone(&received);
    let filter = MqttTopicFilter::new("taktora/+/pubsub").unwrap();
    let _sub = session
        .subscribe(
            &filter,
            Box::new(move |bytes: &[u8]| {
                let tick: Tick = JsonCodec.decode(bytes).unwrap();
                sink_received.lock().unwrap().push(tick);
            }),
        )
        .await
        .unwrap();

    // Publish a JSON-encoded Tick on a concrete topic.
    let topic = MqttTopic::new("taktora/examples/pubsub").unwrap();
    let routing = MqttRouting::new(topic, MqttQos::AtLeastOnce);
    let tick = Tick {
        seq: 7,
        label: "hello".to_string(),
    };
    let mut buf = vec![0u8; 256];
    let n = JsonCodec.encode(&tick, &mut buf).unwrap();
    session.publish(&routing, &buf[..n]).await.unwrap();

    // The wildcard subscription's callback fired with the decoded payload.
    assert_eq!(*received.lock().unwrap(), vec![tick.clone()]);

    // The mock records every publish for assertion.
    let published = session.published();
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].0, "taktora/examples/pubsub");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn non_matching_subscription_does_not_fire() {
    let session = MockMqttSession::new();
    let hits = Arc::new(Mutex::new(0u32));
    let sink_hits = Arc::clone(&hits);
    let filter = MqttTopicFilter::new("other/#").unwrap();
    let _sub = session
        .subscribe(
            &filter,
            Box::new(move |_bytes: &[u8]| {
                *sink_hits.lock().unwrap() += 1;
            }),
        )
        .await
        .unwrap();

    let routing = MqttRouting::new(MqttTopic::new("taktora/x").unwrap(), MqttQos::AtMostOnce);
    session.publish(&routing, b"{}").await.unwrap();

    assert_eq!(*hits.lock().unwrap(), 0);
}
