//! Compile-time witness that `J1939Connector` implements `Connector`
//! with the framework's required associated types; `name()` is
//! `"j1939"`; a fresh connector's `health()` is `Connecting`.

#![allow(clippy::doc_markdown)]

use std::sync::Arc;

use taktora_connector_can::{CanIface, CanInterfaceLike, MockCanInterface};
use taktora_connector_codec::JsonCodec;
use taktora_connector_core::{ChannelDescriptor, ConnectorHealthKind};
use taktora_connector_host::Connector;
use taktora_connector_j1939::{
    J1939Connector, J1939ConnectorOptions, J1939Interface, J1939Routing, J1939State, Pgn,
    TransportClass,
};
use taktora_connector_transport_iox::{ChannelReader, ChannelWriter};

#[test]
fn j1939_connector_implements_connector_with_required_associated_types() {
    let iface = CanIface::new("vcan0").unwrap();
    let opts = J1939ConnectorOptions::builder()
        .interface(J1939Interface::new(iface, 0x11))
        .build();
    let state = Arc::new(J1939State::new(opts));
    let driver = MockCanInterface::new(iface);
    let connector =
        J1939Connector::<MockCanInterface, JsonCodec>::new(state, vec![driver], JsonCodec::new())
            .expect("construct J1939Connector");

    // Associated-type witnesses.
    fn requires_routing<T: taktora_connector_core::Routing>() {}
    fn requires_codec<T: taktora_connector_core::PayloadCodec>() {}
    requires_routing::<<J1939Connector<MockCanInterface, JsonCodec> as Connector>::Routing>();
    requires_codec::<<J1939Connector<MockCanInterface, JsonCodec> as Connector>::Codec>();

    // name() + fresh health().
    assert_eq!(connector.name(), "j1939");
    assert_eq!(connector.health().kind(), ConnectorHealthKind::Connecting);

    // create_writer / create_reader return concrete handles, not boxed
    // trait objects.
    fn returns_concrete_writer(_w: ChannelWriter<u32, JsonCodec, 8>) {}
    fn returns_concrete_reader(_r: ChannelReader<u32, JsonCodec, 8>) {}

    let routing = J1939Routing {
        pgn: Pgn::new(59904).unwrap(),
        source_addr: None,
        dest_addr: None,
        transport: TransportClass::SingleFrame,
        priority: 6,
    };
    let desc = ChannelDescriptor::<J1939Routing, 8>::new("trait_surface.single", routing).unwrap();
    let writer = connector.create_writer::<u32, 8>(&desc).unwrap();
    let reader = connector.create_reader::<u32, 8>(&desc).unwrap();
    returns_concrete_writer(writer);
    returns_concrete_reader(reader);

    // Smoke: MockCanInterface satisfies the reused driver trait.
    fn requires_can_interface_like<T: CanInterfaceLike>() {}
    requires_can_interface_like::<MockCanInterface>();
}
