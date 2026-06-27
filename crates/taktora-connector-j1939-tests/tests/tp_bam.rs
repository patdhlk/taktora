//! Layer-1 end-to-end coverage for the J1939 BAM transport protocol
//! (issue #123 / FEAT_0098). `REQ_0892`, `REQ_0895`, `REQ_0896`.
//!
//! * TEST_0888 — BAM round-trip via `MockJ1939Interface`: an outbound
//!   payload is segmented into TP.CM(BAM) + TP.DT frames, looped back
//!   through the mock bus, reassembled INBOUND, and delivered on the
//!   matching PGN channel.
//! * TEST_0891 — a TP session whose next TP.DT is withheld past T1
//!   aborts on the timer and emits a `HealthEvent` (observed via
//!   `HealthSubscription`) rather than silently dropping.
//! * TEST_0892 — concurrent inbound TP sessions are bounded: opening one
//!   more than the configured cap refuses the excess with a connection
//!   abort + `HealthEvent` and allocates no extra session.
//!
//! Each test drives the dispatcher synchronously via
//! `dispatch_one_iteration_tp`, which steps a caller-owned `TpEngine`
//! with an explicit `now` so the J1939-21 timers advance deterministically
//! without real sleeps.

#![allow(clippy::doc_markdown)]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use iceoryx2::node::Node;
use iceoryx2::prelude::{NodeBuilder, ipc};
use taktora_connector_can::{
    CanHealthMonitor, CanIface, ChannelBinding, Direction, IoxInboundPublish, IoxOutboundDrain,
    MockCanInterface,
};
use taktora_connector_codec::JsonCodec;
use taktora_connector_core::{ChannelDescriptor, ConnectorHealthKind, ExponentialBackoff};
use taktora_connector_j1939::{
    J1939Registry, J1939Routing, Pgn, TpAbortReason, TpEngine, TpEvent, TpTimers,
    dispatch_one_iteration_tp,
};
use taktora_connector_transport_iox::{ChannelReader, ChannelWriter, ServiceFactory};

const SA: u8 = 0x11;
const RECV_TICK: Duration = Duration::from_millis(5);

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

/// Channel sizing for the TP channels under test (the const generic `N`
/// must equal the routing's `Tp { max_len }`).
const TP_N: usize = 1785;

/// TEST_0888 — BAM round-trip. A `Vec<u8>` payload (>8 bytes so it must
/// use BAM) is written to an outbound TP channel, segmented into
/// TP.CM(BAM) + TP.DT frames, looped back through the mock bus,
/// reassembled, and delivered to the matching inbound TP channel.
#[tokio::test]
async fn test_0888_bam_round_trip() {
    let node = make_node();
    let factory = ServiceFactory::new(&node);
    let registry = Arc::new(Mutex::new(J1939Registry::with_capacity(2)));
    let health = Arc::new(CanHealthMonitor::new(&[iface("vcan0")]));

    // PGN 65260 (PDU2 broadcast, 0xFECC) is a natural BAM-transported PGN.
    // Same PGN both directions; the inbound SA filter is the iface SA so
    // we prove the segmenter's source address round-trips.
    let routing = J1939Routing::tp(pgn(65260), TP_N).with_source_addr(SA);
    let reader = open_inbound::<Vec<u8>, TP_N>(&factory, &registry, "t888.in", routing);
    let writer = open_outbound::<Vec<u8>, TP_N>(&factory, &registry, "t888.out", routing);

    let mut driver = MockCanInterface::new(iface("vcan0"));
    let mut policy = ExponentialBackoff::default();
    let mut tp = TpEngine::new(TpTimers::default(), 8);
    let now = Instant::now();

    // 50-byte payload → JSON encoding (~140 bytes) is well within
    // 9..=1785 and forces a multi-packet BAM (> 8 bytes).
    let msg: Vec<u8> = (0..50u8).collect();
    writer.send(&msg).expect("send");

    // First iteration segments + sends every frame onto the loopback bus
    // and consumes the first (TP.CM). Drive enough iterations to consume
    // the TP.CM and all TP.DT frames (one frame recv'd per iteration).
    let mut total_tx = 0usize;
    let mut completed = 0usize;
    let mut delivered = 0usize;
    for _ in 0..256 {
        let outcome = dispatch_one_iteration_tp(
            &iface("vcan0"),
            SA,
            &mut driver,
            &registry,
            &health,
            &mut tp,
            &mut policy,
            RECV_TICK,
            now,
        )
        .await
        .unwrap();
        total_tx += outcome.tx_sent;
        completed += outcome.tp_completed;
        delivered += outcome.inbound_publishes;
        if completed > 0 {
            break;
        }
    }

    // 200 bytes of JSON → ceil(len/7) DT frames + 1 CM frame, all sent.
    assert!(total_tx > 1, "TP.CM + at least one TP.DT were sent");
    assert_eq!(completed, 1, "exactly one BAM session reassembled");
    assert_eq!(delivered, 1, "reassembled payload published to one channel");
    assert_eq!(
        tp.active_inbound_sessions(),
        0,
        "session freed on completion"
    );

    let received = reader.try_recv().unwrap().expect("payload delivered");
    assert_eq!(
        received.value, msg,
        "reassembled payload matches the source"
    );
}

/// TEST_0891 — TP timeout → health. Open a BAM session and accept the
/// first TP.DT, then withhold the next packet past T1. The session aborts
/// on the timer (`TpAbortReason::Timeout`) and a `HealthEvent` is emitted
/// — observed both on the `IterationOutcome` and via a health
/// subscription — rather than silently dropping.
#[tokio::test]
async fn test_0891_tp_timeout_emits_health_event() {
    let node = make_node();
    let factory = ServiceFactory::new(&node);
    let registry = Arc::new(Mutex::new(J1939Registry::with_capacity(1)));
    let health = Arc::new(CanHealthMonitor::new(&[iface("vcan0")]));
    let sub = health.subscribe();

    let routing = J1939Routing::tp(pgn(65260), TP_N);
    let _reader = open_inbound::<Vec<u8>, TP_N>(&factory, &registry, "t891.in", routing);

    // A standalone engine segments a payload so we can replay only the
    // TP.CM and the first TP.DT, then never feed the rest.
    let seg = TpEngine::new(TpTimers::default(), 8);
    let payload: Vec<u8> = (0..50u8).collect();
    let frames = seg
        .segment_outbound(pgn(65260), 7, 0x22, &payload)
        .expect("segment");
    let cm = &frames[0];
    let dt1 = &frames[1];

    let mut driver = MockCanInterface::new(iface("vcan0"));
    let mut policy = ExponentialBackoff::default();
    let timers = TpTimers::default();
    let mut tp = TpEngine::new(timers, 8);
    let t0 = Instant::now();

    // Inject TP.CM then TP.DT #1 via the driver's own loopback, one per
    // dispatcher iteration, both at t0.
    inject_tp(&driver, cm);
    let _ = dispatch_one_iteration_tp(
        &iface("vcan0"),
        SA,
        &mut driver,
        &registry,
        &health,
        &mut tp,
        &mut policy,
        RECV_TICK,
        t0,
    )
    .await
    .unwrap();

    inject_tp(&driver, dt1);
    let _ = dispatch_one_iteration_tp(
        &iface("vcan0"),
        SA,
        &mut driver,
        &registry,
        &health,
        &mut tp,
        &mut policy,
        RECV_TICK,
        t0,
    )
    .await
    .unwrap();

    assert_eq!(
        tp.active_inbound_sessions(),
        1,
        "session is open after first DT"
    );

    // Drain any bring-up / earlier events so we assert on the abort edge.
    while sub.try_recv().is_ok() {}

    // Advance the clock past T1 with no further TP.DT — the timer fires.
    let after_t1 = t0 + timers.t1 + Duration::from_millis(10);
    let outcome = dispatch_one_iteration_tp(
        &iface("vcan0"),
        SA,
        &mut driver,
        &registry,
        &health,
        &mut tp,
        &mut policy,
        RECV_TICK,
        after_t1,
    )
    .await
    .unwrap();

    assert_eq!(outcome.tp_aborted, 1, "session aborted on the T1 timer");
    assert_eq!(tp.active_inbound_sessions(), 0, "aborted session freed");
    match outcome.tp_events.first() {
        Some(TpEvent::Aborted { reason, .. }) => {
            assert_eq!(*reason, TpAbortReason::Timeout);
        }
        other => panic!("expected an Aborted event, got {other:?}"),
    }

    // The abort surfaced as a HealthEvent (not a silent drop).
    let evt = sub
        .try_recv()
        .expect("a HealthEvent was emitted for the TP abort");
    assert_eq!(evt.to.kind(), ConnectorHealthKind::Degraded);
}

/// TEST_0892 — concurrent inbound TP sessions bounded. With a cap of 2,
/// opening a third BAM session (distinct source address) is refused with
/// a connection abort (`TpAbortReason::Resources`) + `HealthEvent`, and
/// no third session is allocated.
#[tokio::test]
async fn test_0892_concurrent_sessions_bounded() {
    let node = make_node();
    let factory = ServiceFactory::new(&node);
    let registry = Arc::new(Mutex::new(J1939Registry::with_capacity(1)));
    let health = Arc::new(CanHealthMonitor::new(&[iface("vcan0")]));
    let sub = health.subscribe();

    let routing = J1939Routing::tp(pgn(65260), TP_N);
    let _reader = open_inbound::<Vec<u8>, TP_N>(&factory, &registry, "t892.in", routing);

    let mut driver = MockCanInterface::new(iface("vcan0"));
    let mut policy = ExponentialBackoff::default();
    // Cap of 2 concurrent inbound sessions.
    let mut tp = TpEngine::new(TpTimers::default(), 2);
    let now = Instant::now();

    // Three distinct sources each announce a BAM for the same PGN.
    let seg = TpEngine::new(TpTimers::default(), 8);
    let mut refused = 0usize;
    for sa in [0x30u8, 0x31, 0x32] {
        let frames = seg
            .segment_outbound(pgn(65260), 7, sa, &(0..20u8).collect::<Vec<_>>())
            .expect("segment");
        inject_tp(&driver, &frames[0]); // TP.CM only — open the session.
        let outcome = dispatch_one_iteration_tp(
            &iface("vcan0"),
            SA,
            &mut driver,
            &registry,
            &health,
            &mut tp,
            &mut policy,
            RECV_TICK,
            now,
        )
        .await
        .unwrap();
        refused += outcome.tp_refused;
        if let Some(TpEvent::SessionRefused { reason, .. }) = outcome.tp_events.first() {
            assert_eq!(*reason, TpAbortReason::Resources);
        }
    }

    assert_eq!(
        refused, 1,
        "exactly the third (over-cap) session is refused"
    );
    assert_eq!(
        tp.active_inbound_sessions(),
        2,
        "no unbounded allocation: session count stays at the cap"
    );

    // The refusal surfaced as a HealthEvent.
    let saw_degraded = std::iter::from_fn(|| sub.try_recv().ok())
        .any(|e| e.to.kind() == ConnectorHealthKind::Degraded);
    assert!(
        saw_degraded,
        "session refusal emitted a Degraded HealthEvent"
    );
}

/// Inject one segmented TP frame onto the driver's loopback bus by
/// encoding its 29-bit id and pushing a classical data frame.
fn inject_tp(driver: &MockCanInterface, frame: &taktora_connector_j1939::TpOutFrame) {
    use taktora_connector_can::{CanData, CanFdFlags, CanFrameKind, CanId};
    let raw = taktora_connector_j1939::encode_extended_id(
        frame.wire_pgn,
        frame.priority,
        frame.source_addr,
        frame.dest_addr,
    );
    let id = CanId::extended(raw).expect("extended id");
    let data = CanData::new(
        id,
        CanFrameKind::Classical,
        CanFdFlags::empty(),
        &frame.data,
    )
    .expect("frame");
    driver.inject_frame(data);
}
