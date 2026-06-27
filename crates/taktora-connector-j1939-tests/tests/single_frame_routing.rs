//! Layer-1 end-to-end coverage for the J1939 connector tracer bullet.
//!
//! * TEST_0886 — 29-bit id decode + PGN/SA/DA demux through
//!   `MockJ1939Interface` (PDU1 dest-specific AND PDU2 broadcast).
//! * TEST_0887 — transport-class `N` validation at `create_writer` /
//!   `create_reader`.
//! * TEST_0895 — mock harness round-trips single-frame PGNs with no
//!   kernel CAN module and no socketcan dependency.
//!
//! Each test wires the moving parts directly (iceoryx2 `Node`,
//! `ServiceFactory`, a hand-built `J1939Registry`, a
//! `MockJ1939Interface`, and `dispatch_one_iteration`) so each
//! dispatcher iteration is synchronous and observable.

#![allow(clippy::doc_markdown)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use iceoryx2::node::Node;
use iceoryx2::prelude::{NodeBuilder, ipc};
use taktora_connector_can::{
    CanHealthMonitor, CanIface, ChannelBinding, Direction, IoxInboundPublish, IoxOutboundDrain,
    MockCanInterface,
};
use taktora_connector_codec::JsonCodec;
use taktora_connector_core::{ChannelDescriptor, ExponentialBackoff};
use taktora_connector_host::Connector;
use taktora_connector_j1939::{
    J1939Connector, J1939ConnectorOptions, J1939Interface, J1939Registry, J1939Routing, J1939State,
    MockJ1939Interface, Pgn, TransportClass, dispatch_one_iteration,
};
use taktora_connector_transport_iox::{ChannelReader, ChannelWriter, ServiceFactory};

const SA: u8 = 0x11;

fn make_node() -> Node<ipc::Service> {
    NodeBuilder::new()
        .create::<ipc::Service>()
        .expect("create iceoryx2 node")
}

fn iface(name: &str) -> CanIface {
    CanIface::new(name).unwrap()
}

fn pgn(v: u32) -> Pgn {
    Pgn::new(v).unwrap()
}

fn open_outbound<T, const N: usize>(
    factory: &ServiceFactory<'_>,
    registry: &Mutex<J1939Registry>,
    service_name: &str,
    routing: J1939Routing,
) -> ChannelWriter<T, JsonCodec, N>
where
    T: serde::Serialize + 'static,
{
    let desc =
        ChannelDescriptor::<J1939Routing, N>::new(service_name.to_string(), routing).unwrap();
    let writer = factory
        .create_writer::<T, _, _, N>(&desc, JsonCodec::new())
        .expect("plugin writer");
    let raw_reader = factory
        .create_raw_reader_named::<N>(service_name)
        .expect("gateway raw reader");
    registry.lock().unwrap().register(
        service_name.to_string(),
        routing,
        Direction::Outbound,
        ChannelBinding::Outbound(Box::new(IoxOutboundDrain::<N>::new(raw_reader))),
    );
    writer
}

fn open_inbound<T, const N: usize>(
    factory: &ServiceFactory<'_>,
    registry: &Mutex<J1939Registry>,
    service_name: &str,
    routing: J1939Routing,
) -> ChannelReader<T, JsonCodec, N>
where
    T: serde::de::DeserializeOwned + 'static,
{
    let desc =
        ChannelDescriptor::<J1939Routing, N>::new(service_name.to_string(), routing).unwrap();
    let reader = factory
        .create_reader::<T, _, _, N>(&desc, JsonCodec::new())
        .expect("plugin reader");
    let raw_writer = factory
        .create_raw_writer_named::<N>(service_name)
        .expect("gateway raw writer");
    registry.lock().unwrap().register(
        service_name.to_string(),
        routing,
        Direction::Inbound,
        ChannelBinding::Inbound(Box::new(IoxInboundPublish::<N>::new(raw_writer))),
    );
    reader
}

/// TEST_0886 — PDU1 destination-specific frame demuxes only to channels
/// whose PGN (and optional SA/DA filters) match; `None` is a wildcard.
#[tokio::test]
async fn test_0886_pdu1_demux_with_address_filters() {
    let node = make_node();
    let factory = ServiceFactory::new(&node);
    let registry = Arc::new(Mutex::new(J1939Registry::with_capacity(4)));
    let health = Arc::new(CanHealthMonitor::new(&[iface("vcan0")]));

    // Request PGN 59904 (PDU1). Three readers:
    //  - wildcard (matches)
    //  - source-addr filter 0x11 (matches the injected SA)
    //  - source-addr filter 0x99 (does NOT match)
    let wildcard = J1939Routing::single_frame(pgn(59904));
    let sa_match = J1939Routing::single_frame(pgn(59904)).with_source_addr(0x11);
    let sa_miss = J1939Routing::single_frame(pgn(59904)).with_source_addr(0x99);

    let r_wild = open_inbound::<u8, 8>(&factory, &registry, "t886.wild.in", wildcard);
    let r_match = open_inbound::<u8, 8>(&factory, &registry, "t886.match.in", sa_match);
    let r_miss = open_inbound::<u8, 8>(&factory, &registry, "t886.miss.in", sa_miss);

    let mut harness = MockJ1939Interface::new(iface("vcan0"));
    let mut policy = ExponentialBackoff::default();

    // Inject a JSON-encoded u8 (so the reader's JsonCodec round-trips).
    let json = serde_json::to_vec(&42u8).unwrap();
    let raw = harness
        .inject_j1939(pgn(59904), 6, 0x11, Some(0x21), &json)
        .expect("inject");
    assert_eq!(
        raw, 0x18EA_2111,
        "encoded PDU1 id matches the hand-computed value"
    );

    let outcome = dispatch_one_iteration(
        &iface("vcan0"),
        SA,
        harness.driver_mut(),
        &registry,
        &health,
        &mut policy,
        Duration::from_millis(200),
    )
    .await
    .unwrap();

    // Two readers match (wildcard + SA 0x11); the SA 0x99 reader does not.
    assert_eq!(outcome.inbound_publishes, 2);
    assert_eq!(r_wild.try_recv().unwrap().unwrap().value, 42u8);
    assert_eq!(r_match.try_recv().unwrap().unwrap().value, 42u8);
    assert!(r_miss.try_recv().unwrap().is_none());
}

/// TEST_0886 — PDU2 broadcast frame demuxes by group-extension PGN; a
/// destination-address filter never matches a broadcast frame.
#[tokio::test]
async fn test_0886_pdu2_broadcast_demux() {
    let node = make_node();
    let factory = ServiceFactory::new(&node);
    let registry = Arc::new(Mutex::new(J1939Registry::with_capacity(4)));
    let health = Arc::new(CanHealthMonitor::new(&[iface("vcan0")]));

    // PGN 65270 is PDU2 (PF 0xFE). A wildcard reader matches; a reader
    // with a dest-addr filter must NOT match (broadcast has no DA).
    let bcast = J1939Routing::single_frame(pgn(65270));
    let with_da = J1939Routing::single_frame(pgn(65270)).with_dest_addr(0x21);
    // A different-PGN reader must not match.
    let other = J1939Routing::single_frame(pgn(59904));

    let r_bcast = open_inbound::<u8, 8>(&factory, &registry, "t886b.bcast.in", bcast);
    let r_da = open_inbound::<u8, 8>(&factory, &registry, "t886b.da.in", with_da);
    let r_other = open_inbound::<u8, 8>(&factory, &registry, "t886b.other.in", other);

    let mut harness = MockJ1939Interface::new(iface("vcan0"));
    let mut policy = ExponentialBackoff::default();

    let json = serde_json::to_vec(&7u8).unwrap();
    let raw = harness
        .inject_j1939(pgn(65270), 3, 0x80, None, &json)
        .expect("inject");
    assert_eq!(
        raw, 0x0CFE_F680,
        "encoded PDU2 id matches the hand-computed value"
    );

    let outcome = dispatch_one_iteration(
        &iface("vcan0"),
        SA,
        harness.driver_mut(),
        &registry,
        &health,
        &mut policy,
        Duration::from_millis(200),
    )
    .await
    .unwrap();

    assert_eq!(outcome.inbound_publishes, 1);
    assert_eq!(r_bcast.try_recv().unwrap().unwrap().value, 7u8);
    assert!(r_da.try_recv().unwrap().is_none());
    assert!(r_other.try_recv().unwrap().is_none());
}

/// TEST_0887 — channel `N` is validated against the routing's transport
/// class at create time; a match is accepted, a mismatch is rejected
/// with `ConnectorError::Configuration`.
#[test]
fn test_0887_transport_class_n_validation() {
    let ifc = iface("vcan0");
    let opts = J1939ConnectorOptions::builder()
        .interface(J1939Interface::new(ifc, SA))
        .build();
    let state = Arc::new(J1939State::new(opts));
    let driver = MockCanInterface::new(ifc);
    let connector =
        J1939Connector::<MockCanInterface, JsonCodec>::new(state, vec![driver], JsonCodec::new())
            .expect("construct connector");

    // SingleFrame → N must be 8.
    let sf = J1939Routing::single_frame(pgn(59904));
    let desc_ok = ChannelDescriptor::<J1939Routing, 8>::new("t887.sf.ok", sf).unwrap();
    assert!(connector.create_writer::<u8, 8>(&desc_ok).is_ok());
    assert!(connector.create_reader::<u8, 8>(&desc_ok).is_ok());

    let desc_bad = ChannelDescriptor::<J1939Routing, 16>::new("t887.sf.bad", sf).unwrap();
    match connector.create_writer::<u8, 16>(&desc_bad) {
        Err(taktora_connector_core::ConnectorError::Configuration(_)) => {}
        Err(other) => panic!("SingleFrame with N=16 must be Configuration, got {other:?}"),
        Ok(_) => panic!("SingleFrame with N=16 must be rejected"),
    }

    // Tp { max_len: 100 } → N must be 100.
    let tp = J1939Routing::tp(pgn(60416), 100);
    assert_eq!(tp.transport, TransportClass::Tp { max_len: 100 });
    let desc_tp_ok = ChannelDescriptor::<J1939Routing, 100>::new("t887.tp.ok", tp).unwrap();
    assert!(connector.create_reader::<Vec<u8>, 100>(&desc_tp_ok).is_ok());

    let desc_tp_bad = ChannelDescriptor::<J1939Routing, 8>::new("t887.tp.bad", tp).unwrap();
    match connector.create_reader::<Vec<u8>, 8>(&desc_tp_bad) {
        Err(taktora_connector_core::ConnectorError::Configuration(_)) => {}
        Err(other) => panic!("Tp{{max_len:100}} with N=8 must be Configuration, got {other:?}"),
        Ok(_) => panic!("Tp{{max_len:100}} with N=8 must be rejected"),
    }
}

/// TEST_0895 — the mock harness round-trips a single-frame PGN with no
/// kernel CAN module: inject a raw J1939 frame, observe the decoded
/// payload on the matching channel.
#[tokio::test]
async fn test_0895_mock_harness_inject_round_trip() {
    let node = make_node();
    let factory = ServiceFactory::new(&node);
    let registry = Arc::new(Mutex::new(J1939Registry::with_capacity(1)));
    let health = Arc::new(CanHealthMonitor::new(&[iface("vcan0")]));

    let routing = J1939Routing::single_frame(pgn(59904));
    let reader = open_inbound::<String, 8>(&factory, &registry, "t895.in", routing);

    let mut harness = MockJ1939Interface::new(iface("vcan0"));
    let mut policy = ExponentialBackoff::default();

    let payload = "hi".to_string();
    let json = serde_json::to_vec(&payload).unwrap();
    harness
        .inject_j1939(pgn(59904), 6, 0x11, Some(0x21), &json)
        .expect("inject");

    let outcome = dispatch_one_iteration(
        &iface("vcan0"),
        SA,
        harness.driver_mut(),
        &registry,
        &health,
        &mut policy,
        Duration::from_millis(200),
    )
    .await
    .unwrap();
    assert_eq!(outcome.inbound_publishes, 1);
    assert_eq!(reader.try_recv().unwrap().unwrap().value, payload);
}

/// TEST_0895 — the mock harness round-trips a single-frame PGN on the TX
/// path: a plugin writer's payload is encoded to a 29-bit frame, looped
/// back through the mock bus, and delivered to the matching reader.
#[tokio::test]
async fn test_0895_mock_harness_tx_loopback_round_trip() {
    let node = make_node();
    let factory = ServiceFactory::new(&node);
    let registry = Arc::new(Mutex::new(J1939Registry::with_capacity(2)));
    let health = Arc::new(CanHealthMonitor::new(&[iface("vcan0")]));

    // Reader registered first (iox pub/sub semantics). Same PGN both
    // directions; the routing carries SA filter so we prove the encoded
    // TX source address (the iface SA) round-trips through decode.
    let routing = J1939Routing::single_frame(pgn(59904)).with_source_addr(SA);
    let reader = open_inbound::<u16, 8>(&factory, &registry, "t895tx.in", routing);
    let writer = open_outbound::<u16, 8>(&factory, &registry, "t895tx.out", routing);

    let mut driver = MockCanInterface::new(iface("vcan0"));
    let mut policy = ExponentialBackoff::default();

    writer.send(&513u16).expect("send");

    let outcome = dispatch_one_iteration(
        &iface("vcan0"),
        SA,
        &mut driver,
        &registry,
        &health,
        &mut policy,
        Duration::from_millis(200),
    )
    .await
    .unwrap();
    assert_eq!(outcome.tx_sent, 1);
    assert_eq!(outcome.inbound_publishes, 1);
    assert_eq!(reader.try_recv().unwrap().unwrap().value, 513u16);
}
