//! `REQ_0250` — `MqttConnector<C>` implements `Connector` with the
//! framework's required associated types, and `create_writer` /
//! `create_reader` return concrete iceoryx2 handles. Compile-time API
//! surface check plus a light runtime smoke.

use std::sync::Arc;

use taktora_connector_codec::JsonCodec;
use taktora_connector_core::{ChannelDescriptor, ConnectorHealthKind};
use taktora_connector_host::Connector;
use taktora_connector_mqtt::{
    MockMqttSession, MqttConnector, MqttConnectorOptions, MqttQos, MqttRouting, MqttState,
    MqttTopic,
};
use taktora_connector_transport_iox::{ChannelReader, ChannelWriter};

const N: usize = 128;

#[test]
fn mqtt_connector_implements_connector_with_required_associated_types() {
    let state = Arc::new(MqttState::new(MqttConnectorOptions::builder().build()));
    let session = Arc::new(MockMqttSession::new());
    let connector = MqttConnector::new(state, session, JsonCodec).expect("construct MqttConnector");

    // Associated-type witnesses (REQ_0250: Routing = MqttRouting).
    fn requires_routing<T: taktora_connector_core::Routing>() {}
    fn requires_codec<T: taktora_connector_core::PayloadCodec>() {}
    requires_routing::<<MqttConnector<JsonCodec> as Connector>::Routing>();
    requires_codec::<<MqttConnector<JsonCodec> as Connector>::Codec>();

    // name + health surface.
    assert_eq!(connector.name(), "mqtt");
    assert_eq!(connector.health().kind(), ConnectorHealthKind::Connecting);
    let _sub = connector.subscribe_health(); // smoke

    // create_writer / create_reader return concrete handles, not boxed
    // trait objects (REQ_0223). The types appear in these signatures to
    // assert that statically.
    fn returns_concrete_writer(_w: ChannelWriter<u32, JsonCodec, N>) {}
    fn returns_concrete_reader(_r: ChannelReader<u32, JsonCodec, N>) {}

    let routing = MqttRouting::new(
        MqttTopic::new("taktora/trait/surface").unwrap(),
        MqttQos::AtLeastOnce,
    );
    let desc = ChannelDescriptor::<MqttRouting, N>::new("taktora.trait.surface", routing).unwrap();
    let writer = connector.create_writer::<u32, N>(&desc).unwrap();
    let reader = connector.create_reader::<u32, N>(&desc).unwrap();
    returns_concrete_writer(writer);
    returns_concrete_reader(reader);
}
