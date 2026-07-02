//! Inbound path end-to-end via `MockMqttSession` (`REQ_0254`, `REQ_0987`,
//! `REQ_0986`, `REQ_0985`).
//!
//! A simulated broker PUBLISH (`MockMqttSession::deliver_inbound`) is
//! matched locally by the gateway against every registered channel filter
//! and fanned out to each matching `ChannelReader` (`ADR_0129`). Broker
//! SUBSCRIBEs are deduplicated + reference-counted, and replayed on
//! reconnect.
//!
//! iceoryx2 services are process-global, so these run serially
//! (`--test-threads=1`).

use std::sync::Arc;
use std::time::{Duration, Instant};

use taktora_connector_codec::JsonCodec;
use taktora_connector_core::{ChannelDescriptor, PayloadCodec};
use taktora_connector_host::Connector;
use taktora_connector_mqtt::{
    MockMqttSession, MqttConnector, MqttConnectorOptions, MqttQos, MqttRouting, MqttState,
    MqttTopic, MqttTopicFilter,
};
use taktora_connector_transport_iox::ChannelReader;

const N: usize = 256;

fn make_connector(session: &Arc<MockMqttSession>) -> MqttConnector<JsonCodec, MockMqttSession> {
    let state = Arc::new(MqttState::new(MqttConnectorOptions::builder().build()));
    MqttConnector::new(state, Arc::clone(session), JsonCodec).expect("construct MqttConnector")
}

/// Build an inbound routing whose subscription filter is `filter` (which
/// may carry wildcards). The concrete placeholder topic is never used on
/// the inbound path.
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

/// Poll a reader for one value with a deadline.
fn recv_one(reader: &ChannelReader<u32, JsonCodec, N>) -> Option<u32> {
    let deadline = Instant::now() + Duration::from_millis(500);
    while Instant::now() < deadline {
        if let Ok(Some(env)) = reader.try_recv() {
            return Some(env.value);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    None
}

/// `REQ_0254` / `REQ_0987`: an inbound PUBLISH on a concrete topic is
/// matched locally against ALL registered channel filters and delivered to
/// every matching `ChannelReader` — including overlapping wildcard filters
/// — and NOT to a non-matching reader.
#[test]
fn inbound_publish_fans_out_to_every_matching_reader() {
    let session = Arc::new(MockMqttSession::new());
    let connector = make_connector(&session);

    // Two filters that both match `robot/arm/telemetry`, one that does not.
    let single = ChannelDescriptor::<MqttRouting, N>::new(
        "chan.single".to_string(),
        reader_routing("robot/+/telemetry"),
    )
    .unwrap();
    let multi = ChannelDescriptor::<MqttRouting, N>::new(
        "chan.multi".to_string(),
        reader_routing("robot/arm/#"),
    )
    .unwrap();
    let other = ChannelDescriptor::<MqttRouting, N>::new(
        "chan.other".to_string(),
        reader_routing("robot/leg/telemetry"),
    )
    .unwrap();

    let r_single = connector.create_reader::<u32, N>(&single).unwrap();
    let r_multi = connector.create_reader::<u32, N>(&multi).unwrap();
    let r_other = connector.create_reader::<u32, N>(&other).unwrap();

    // Simulate the broker delivering an inbound PUBLISH.
    let topic = MqttTopic::new("robot/arm/telemetry").unwrap();
    session.deliver_inbound(&topic, &encode(7));

    assert_eq!(recv_one(&r_single), Some(7), "single-level wildcard matches");
    assert_eq!(recv_one(&r_multi), Some(7), "multi-level wildcard matches");
    // The non-matching reader must not receive anything.
    assert!(
        r_other.try_recv().unwrap().is_none(),
        "non-matching filter must not receive the PUBLISH"
    );
}
