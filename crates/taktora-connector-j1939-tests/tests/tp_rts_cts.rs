//! Layer-1 end-to-end coverage for the J1939 RTS/CTS connection-mode
//! transport protocol (issue #124 / FEAT_0098). `REQ_0893`, `REQ_0895`.
//!
//! * TEST_0889 — RTS/CTS round-trip: a full
//!   `RTS → CTS → TP.DT → (CTS → TP.DT)* → EndOfMsgAck` exchange driven
//!   through two `TpEngine`s (sender + receiver) with the receiver's CTS
//!   flow control granting packets across MULTIPLE windows. Asserts exact
//!   reassembly of a 9..=1785-byte payload, then that a `Conn_Abort`
//!   surfaces as a `HealthEvent` through the dispatcher path.
//!
//! ## Why the engine is driven directly for the handshake
//!
//! `MockCanInterface` is a single-queue loopback: a connector's own sends
//! loop back to its own RX, so a two-node RTS/CTS handshake cannot
//! complete by pure loopback in one connector. The connection-mode state
//! machine is therefore exercised by stepping BOTH endpoints through the
//! clock-stepped `TpEngine` API deterministically — the robust approach
//! the engine was designed for. The dispatcher-level loopback is then used
//! for the abort → `HealthEvent` assertion (which a single connector can
//! observe end-to-end).

#![allow(clippy::doc_markdown)]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use iceoryx2::node::Node;
use iceoryx2::prelude::{NodeBuilder, ipc};
use taktora_connector_can::{
    CanData, CanFdFlags, CanFrameKind, CanHealthMonitor, CanId, CanIface, ChannelBinding,
    Direction, IoxInboundPublish, MockCanInterface,
};
use taktora_connector_codec::JsonCodec;
use taktora_connector_core::{ChannelDescriptor, ConnectorHealthKind, ExponentialBackoff};
use taktora_connector_j1939::{
    CONN_ABORT_CONTROL, CTS_CONTROL, EOMA_CONTROL, J1939Registry, J1939Routing, Pgn, RTS_CONTROL,
    TP_CM_PGN, TpAbortReason, TpEngine, TpEvent, TpOutFrame, TpTimers, decode_extended_id,
    dispatch_one_iteration_tp, encode_extended_id,
};
use taktora_connector_transport_iox::{ChannelReader, ServiceFactory};

const OUR_SA: u8 = 0x11; // the receiving node (us, inbound reassembly)
const PEER_SA: u8 = 0x22; // the transmitting node (peer, outbound segmentation)
const RECV_TICK: Duration = Duration::from_millis(5);
const TP_N: usize = 1785;
/// Window the receiver grants per CTS. Smaller than the total packet count
/// below, so the transfer spans MULTIPLE windows (the heart of TEST_0889).
const CTS_WINDOW: u8 = 4;

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

/// Feed one engine-emitted `TpOutFrame` into another engine's `on_frame`
/// by encoding then decoding its 29-bit id (round-trips the wire form).
fn feed(engine: &mut TpEngine, frame: &TpOutFrame, now: Instant) -> Vec<TpEvent> {
    let dec = decode_extended_id(encode_extended_id(
        frame.wire_pgn,
        frame.priority,
        frame.source_addr,
        frame.dest_addr,
    ));
    engine.on_frame(&dec, &frame.data, now)
}

/// TEST_0889 (engine closed loop) — a full RTS/CTS multi-window transfer.
///
/// A 100-byte payload is `ceil(100/7) = 15` TP.DT packets; with a CTS
/// window of 4 the receiver grants `[1..4], [5..8], [9..12], [13..15]` —
/// four windows, so the flow control genuinely spans MULTIPLE CTS rounds.
/// The two engines are cross-fed until the receiver reassembles the exact
/// payload and emits EndOfMsgAck, which completes the sender's session.
#[test]
fn test_0889_rts_cts_multi_window_round_trip() {
    let transported = pgn(65260);
    let payload: Vec<u8> = (0..100u8).collect();
    let now = Instant::now();

    let mut sender = TpEngine::new(TpTimers::default(), 8);
    let mut receiver = TpEngine::new(TpTimers::default(), 8).with_cts_window(CTS_WINDOW);

    // Sender opens the connection: RTS to the receiver's address.
    let rts = sender
        .start_outbound_connection(transported, 7, PEER_SA, OUR_SA, &payload, now)
        .expect("start outbound connection");
    assert_eq!(rts.len(), 1, "RTS is a single TP.CM frame");
    assert_eq!(rts[0].data[0], RTS_CONTROL);
    assert_eq!(
        rts[0].dest_addr,
        Some(OUR_SA),
        "RTS is destination-specific"
    );
    assert_eq!(
        u16::from_le_bytes([rts[0].data[1], rts[0].data[2]]),
        100,
        "RTS carries the total size"
    );
    assert_eq!(rts[0].data[3], 15, "RTS carries the total packet count");

    // Receiver answers the RTS with its first CTS window.
    assert!(feed(&mut receiver, &rts[0], now).is_empty());
    let mut to_sender = receiver.take_outbound();
    assert_eq!(to_sender.len(), 1);
    assert_eq!(to_sender[0].data[0], CTS_CONTROL);
    assert_eq!(to_sender[0].data[1], CTS_WINDOW, "first window granted");
    assert_eq!(to_sender[0].data[2], 1, "first window starts at packet 1");
    assert_eq!(
        to_sender[0].dest_addr,
        Some(PEER_SA),
        "CTS targets the sender"
    );

    let mut completed: Option<Vec<u8>> = None;
    let mut windows_granted = 1usize; // counted the first CTS above
    let mut bursts = 0usize;

    // Ping-pong: hand CTS/EndOfMsgAck to the sender, hand the resulting
    // TP.DT burst to the receiver, repeat until reassembly completes.
    for _ in 0..64 {
        // Sender consumes the receiver's CTS (or EndOfMsgAck) frames.
        let mut sender_completed_msg = false;
        for f in &to_sender {
            if f.data[0] == EOMA_CONTROL {
                sender_completed_msg = true;
            }
            feed(&mut sender, f, now);
        }
        let to_receiver = sender.take_outbound();
        if to_receiver.is_empty() {
            // Only the EndOfMsgAck remained — both sides are done.
            assert!(
                sender_completed_msg,
                "sender drains with no pending DT only after EndOfMsgAck"
            );
            break;
        }
        bursts += 1;
        assert!(
            to_receiver.len() <= usize::from(CTS_WINDOW),
            "a burst never exceeds the granted window ({} frames)",
            to_receiver.len()
        );

        // Receiver consumes the TP.DT burst; may emit Completed + the next
        // CTS (mid-transfer) or EndOfMsgAck (final packet).
        let mut events = Vec::new();
        for f in &to_receiver {
            events.extend(feed(&mut receiver, f, now));
        }
        for ev in events {
            if let TpEvent::Completed {
                payload: p,
                pgn: g,
                source_addr,
            } = ev
            {
                assert_eq!(g, transported);
                assert_eq!(source_addr, PEER_SA);
                completed = Some(p);
            }
        }
        to_sender = receiver.take_outbound();
        if let Some(next) = to_sender.first() {
            if next.data[0] == CTS_CONTROL {
                windows_granted += 1;
            }
        }
    }

    assert_eq!(
        completed.expect("payload reassembled"),
        payload,
        "reassembled payload matches the source exactly"
    );
    assert!(
        windows_granted >= 2,
        "the transfer spanned MULTIPLE CTS windows (granted {windows_granted})"
    );
    assert_eq!(bursts, 4, "four TP.DT bursts: 4 + 4 + 4 + 3 packets");
    assert_eq!(
        receiver.active_inbound_connections(),
        0,
        "receiver session freed after EndOfMsgAck"
    );
    assert_eq!(
        sender.active_outbound_connections(),
        0,
        "sender session freed after EndOfMsgAck"
    );
}

/// Build a raw TP.CM Conn_Abort frame (PDU1, destination-specific) and
/// inject it onto the mock loopback bus.
fn inject_cm(driver: &MockCanInterface, source: u8, dest: u8, data: [u8; 8]) {
    let raw = encode_extended_id(pgn(TP_CM_PGN), 7, source, Some(dest));
    let id = CanId::extended(raw).expect("extended id");
    let frame =
        CanData::new(id, CanFrameKind::Classical, CanFdFlags::empty(), &data).expect("frame");
    driver.inject_frame(frame);
}

/// TEST_0889 (dispatcher abort → health) — a received TP.Conn_Abort on an
/// open connection-mode session surfaces as a `HealthEvent` (`REQ_0895`)
/// rather than a silent drop.
#[tokio::test]
async fn test_0889_conn_abort_emits_health_event() {
    let node = make_node();
    let factory = ServiceFactory::new(&node);
    let registry = Arc::new(Mutex::new(J1939Registry::with_capacity(1)));
    let health = Arc::new(CanHealthMonitor::new(&[iface("vcan0")]));
    let sub = health.subscribe();

    let routing = J1939Routing::tp(pgn(65260), TP_N).with_source_addr(PEER_SA);
    let _reader: ChannelReader<Vec<u8>, JsonCodec, TP_N> = {
        let desc =
            ChannelDescriptor::<J1939Routing, TP_N>::new("t889.in".to_string(), routing).unwrap();
        let reader = factory
            .create_reader::<Vec<u8>, _, _, TP_N>(&desc, JsonCodec::new())
            .expect("reader");
        let raw_writer = factory
            .create_raw_writer_named::<TP_N>("t889.in")
            .expect("raw writer");
        registry.lock().unwrap().register(
            "t889.in".to_string(),
            routing,
            Direction::Inbound,
            ChannelBinding::Inbound(Box::new(IoxInboundPublish::<TP_N>::new(raw_writer))),
        );
        reader
    };

    // A standalone sender engine produces a real RTS (source = peer, dest
    // = our SA) that opens an inbound connection on the dispatcher's engine.
    let starter = TpEngine::new(TpTimers::default(), 8);
    let payload: Vec<u8> = (0..50u8).collect();
    // start_outbound_connection mutates state, so use a throwaway engine.
    let mut starter = starter;
    let rts = starter
        .start_outbound_connection(pgn(65260), 7, PEER_SA, OUR_SA, &payload, Instant::now())
        .expect("rts");

    let mut driver = MockCanInterface::new(iface("vcan0"));
    let mut policy = ExponentialBackoff::default();
    let mut tp = TpEngine::new(TpTimers::default(), 8).with_cts_window(CTS_WINDOW);
    let t0 = Instant::now();

    // Inject the RTS; the dispatcher opens the session and emits a CTS.
    let raw = encode_extended_id(
        rts[0].wire_pgn,
        rts[0].priority,
        rts[0].source_addr,
        rts[0].dest_addr,
    );
    let id = CanId::extended(raw).unwrap();
    driver.inject_frame(
        CanData::new(
            id,
            CanFrameKind::Classical,
            CanFdFlags::empty(),
            &rts[0].data,
        )
        .unwrap(),
    );
    let _ = dispatch_one_iteration_tp(
        &iface("vcan0"),
        OUR_SA,
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
    assert_eq!(tp.active_inbound_connections(), 1, "RTS opened a session");

    // Drain bring-up / loopback events so we assert on the abort edge.
    while sub.try_recv().is_ok() {}

    // The peer aborts the connection.
    let pgn_v = pgn(65260).value();
    inject_cm(
        &driver,
        PEER_SA,
        OUR_SA,
        [
            CONN_ABORT_CONTROL,
            TpAbortReason::Other.as_u8(),
            0xFF,
            0xFF,
            0xFF,
            (pgn_v & 0xFF) as u8,
            ((pgn_v >> 8) & 0xFF) as u8,
            ((pgn_v >> 16) & 0xFF) as u8,
        ],
    );
    // The mock bus is a single FIFO that also carries our own looped-back
    // CTS, so drive a few iterations until the Conn_Abort is consumed.
    let mut aborted = 0usize;
    let mut abort_reason = None;
    for _ in 0..8 {
        let outcome = dispatch_one_iteration_tp(
            &iface("vcan0"),
            OUR_SA,
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
        aborted += outcome.tp_aborted;
        if let Some(TpEvent::Aborted { reason, .. }) = outcome.tp_events.first() {
            abort_reason = Some(*reason);
        }
        if aborted > 0 {
            break;
        }
    }

    assert_eq!(aborted, 1, "the Conn_Abort aborted the session");
    assert_eq!(
        tp.active_inbound_connections(),
        0,
        "aborted connection-mode session freed"
    );
    assert_eq!(
        abort_reason,
        Some(TpAbortReason::Other),
        "the abort carried the peer's reason code"
    );

    // The abort surfaced as a HealthEvent (not a silent drop) — REQ_0895.
    let evt = sub
        .try_recv()
        .expect("a HealthEvent was emitted for the connection abort");
    assert_eq!(evt.to.kind(), ConnectorHealthKind::Degraded);
}
