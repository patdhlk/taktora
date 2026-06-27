//! Layer-1 end-to-end coverage for the J1939 ETP (Extended Transport
//! Protocol) transport over the large-payload slice channel (issue #125
//! / FEAT_0098). `REQ_0894`, `REQ_0903`, `ADR_0109`, `BB_0101`.
//!
//! TEST_0890 has three deterministic parts, mirroring #124's single-queue
//! mock strategy (a `MockCanInterface` is a single-queue loopback, so a
//! two-node ETP handshake cannot complete by pure loopback in one
//! connector). Together they satisfy the acceptance criterion:
//!
//! 1. **Engine closed-loop ETP round-trip > 1785 B** — two `TpEngine`s
//!    (sender + receiver) are cross-fed a 5000-byte payload that spans
//!    many ETP.DT bursts, exercising the Data-Packet-Offset (DPO) math
//!    for packet numbers beyond 255. Asserts exact reassembly.
//! 2. **Slice-channel delivery of a > 1785 B payload** — the connector's
//!    ETP slice handles (`create_etp_writer` / `create_etp_reader`, bound
//!    to `max_etp_bytes`) round-trip a 5000-byte blob, proving ETP rides
//!    the FEAT_0097 slice channel (length round-trips exactly, not a
//!    fixed `N`).
//! 3. **Bounded abort → HealthEvent** — a small `max_etp_bytes` (8192)
//!    plus an inbound ETP.RTS announcing 20000 bytes is aborted with the
//!    J1939 connection-abort reason (Resources), allocates NO reassembly
//!    buffer, and surfaces as a `HealthEvent` through the dispatcher path.

#![allow(clippy::doc_markdown)]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use taktora_connector_can::{
    CanData, CanFdFlags, CanFrameKind, CanHealthMonitor, CanId, CanIface, MockCanInterface,
};
use taktora_connector_codec::JsonCodec;
use taktora_connector_core::{ConnectorHealthKind, ExponentialBackoff};
use taktora_connector_j1939::{
    ETP_ABORT_CONTROL, ETP_CM_PGN, ETP_CTS_CONTROL, ETP_DPO_CONTROL, ETP_EOMA_CONTROL,
    ETP_RTS_CONTROL, J1939Connector, J1939ConnectorOptions, J1939Interface, J1939Registry,
    J1939Routing, J1939State, Pgn, TpAbortReason, TpEngine, TpEvent, TpOutFrame, TpTimers,
    decode_extended_id, dispatch_one_iteration_tp, encode_extended_id,
};

const OUR_SA: u8 = 0x11; // the receiving node (us, inbound reassembly)
const PEER_SA: u8 = 0x22; // the transmitting node (peer, outbound segmentation)
const RECV_TICK: Duration = Duration::from_millis(5);

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

/// A deterministic but non-trivial test payload of `len` bytes.
fn payload_of(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

/// TEST_0890 (part 1) — engine closed-loop ETP round-trip > 1785 B.
///
/// A 5000-byte payload is `ceil(5000/7) = 715` ETP.DT packets. With the
/// default CTS window of 16 the transfer spans ~45 bursts, so packet
/// numbers far exceed 255 and the DPO offset math is exercised across
/// many bursts. The two engines are cross-fed until the receiver
/// reassembles the exact payload and emits ETP.EndOfMsgAck.
#[test]
fn test_0890_etp_round_trip_engine_closed_loop() {
    let transported = pgn(0x1_2345 & 0x3FFFF); // any valid 18-bit PGN
    let payload = payload_of(5000);
    let now = Instant::now();

    let mut sender = TpEngine::new(TpTimers::default(), 8).with_max_etp_bytes(1 << 20);
    let mut receiver = TpEngine::new(TpTimers::default(), 8).with_max_etp_bytes(1 << 20);

    // Sender opens the connection: ETP.RTS carrying the 32-bit total size.
    let rts = sender
        .start_outbound_etp(transported, 7, PEER_SA, OUR_SA, &payload, now)
        .expect("start outbound ETP");
    assert_eq!(rts.len(), 1, "ETP.RTS is a single ETP.CM frame");
    assert_eq!(rts[0].data[0], ETP_RTS_CONTROL);
    assert_eq!(rts[0].wire_pgn.value(), ETP_CM_PGN);
    assert_eq!(
        rts[0].dest_addr,
        Some(OUR_SA),
        "ETP.RTS is destination-specific"
    );
    assert_eq!(
        u32::from_le_bytes([
            rts[0].data[1],
            rts[0].data[2],
            rts[0].data[3],
            rts[0].data[4]
        ]),
        5000,
        "ETP.RTS carries the 32-bit total size"
    );

    // Receiver answers the RTS with its first CTS window.
    assert!(feed(&mut receiver, &rts[0], now).is_empty());
    let mut to_sender = receiver.take_outbound();
    assert_eq!(to_sender.len(), 1);
    assert_eq!(to_sender[0].data[0], ETP_CTS_CONTROL);

    let mut completed: Option<Vec<u8>> = None;
    let mut dpo_frames = 0usize;
    let mut max_dpo_offset = 0u32;

    for _ in 0..10_000 {
        // Sender consumes the receiver's CTS (or EndOfMsgAck) frames.
        let mut sender_done = false;
        for f in &to_sender {
            if f.data[0] == ETP_EOMA_CONTROL {
                sender_done = true;
            }
            feed(&mut sender, f, now);
        }
        let to_receiver = sender.take_outbound();
        if to_receiver.is_empty() {
            assert!(sender_done, "sender drains empty only after EndOfMsgAck");
            break;
        }
        // Each burst is one ETP.DPO followed by its ETP.DT packets.
        assert_eq!(
            to_receiver[0].data[0], ETP_DPO_CONTROL,
            "DPO precedes each DT burst"
        );
        dpo_frames += 1;
        let offset = u32::from_le_bytes([
            to_receiver[0].data[2],
            to_receiver[0].data[3],
            to_receiver[0].data[4],
            0,
        ]);
        max_dpo_offset = max_dpo_offset.max(offset);

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
    }

    assert_eq!(
        completed.expect("payload reassembled"),
        payload,
        "ETP reassembled payload matches the source exactly"
    );
    assert!(
        dpo_frames >= 2,
        "the transfer spanned MULTIPLE DPO bursts ({dpo_frames})"
    );
    assert!(
        max_dpo_offset > 255,
        "DPO addressed packets beyond 255 (max offset {max_dpo_offset})"
    );
    assert_eq!(
        receiver.active_inbound_etp_sessions(),
        0,
        "receiver session freed"
    );
    assert_eq!(
        sender.active_outbound_etp_sessions(),
        0,
        "sender session freed"
    );
}

/// TEST_0890 (part 2) — a > 1785 B ETP payload rides the FEAT_0097 slice
/// channel through the connector's ETP slice handles, length-exact.
#[test]
fn test_0890_etp_rides_slice_channel() {
    const MAX_ETP: usize = 1 << 20; // 1 MiB ceiling
    let opts = J1939ConnectorOptions::builder()
        .interface(J1939Interface::new(iface("vcan0"), OUR_SA))
        .max_etp_bytes(MAX_ETP)
        .build();
    let state = Arc::new(J1939State::new(opts));
    let driver = MockCanInterface::new(iface("vcan0"));
    // The connector owns its own iceoryx2 node; the slice service it opens
    // is reachable through the same connector's reader handle.
    let connector =
        J1939Connector::<MockCanInterface, JsonCodec>::new(state, vec![driver], JsonCodec::new())
            .expect("construct connector");

    let routing = J1939Routing::etp(pgn(0xFEEE & 0x3FFFF), MAX_ETP).with_source_addr(PEER_SA);
    let writer = connector
        .create_etp_writer("etp.bulk", routing)
        .expect("etp writer");
    let reader = connector
        .create_etp_reader("etp.bulk", routing)
        .expect("etp reader");

    assert_eq!(
        writer.max_payload_bytes(),
        MAX_ETP,
        "the ETP slice writer is bound to max_etp_bytes"
    );

    let payload = payload_of(5000);
    let out = writer.send(&payload).expect("slice send");
    assert_eq!(out.bytes_written, 5000);

    let recv = reader
        .try_recv()
        .expect("recv ok")
        .expect("a slice sample arrived");
    assert_eq!(
        recv.payload().len(),
        5000,
        "the slice length round-trips exactly (not a fixed N)"
    );
    assert_eq!(
        recv.payload(),
        &payload[..],
        "ETP bytes round-trip on the slice channel"
    );
}

/// Build a raw ETP.CM frame (PDU1, destination-specific) and inject it
/// onto the mock loopback bus.
fn inject_etp_cm(driver: &MockCanInterface, source: u8, dest: u8, data: [u8; 8]) {
    let raw = encode_extended_id(pgn(ETP_CM_PGN), 7, source, Some(dest));
    let id = CanId::extended(raw).expect("extended id");
    let frame =
        CanData::new(id, CanFrameKind::Classical, CanFdFlags::empty(), &data).expect("frame");
    driver.inject_frame(frame);
}

/// TEST_0890 (part 3) — an ETP.RTS announcing a size ABOVE `max_etp_bytes`
/// is aborted with the J1939 connection-abort reason (Resources),
/// allocates no reassembly buffer, and surfaces as a `HealthEvent`
/// through the dispatcher path (`REQ_0894`, `REQ_0903`).
#[tokio::test]
async fn test_0890_oversize_etp_aborts_with_health_event() {
    const MAX_ETP: usize = 8192; // small cap so 20000 trips the bound
    let registry = Arc::new(Mutex::new(J1939Registry::with_capacity(1)));
    let health = Arc::new(CanHealthMonitor::new(&[iface("vcan0")]));
    let sub = health.subscribe();

    let mut driver = MockCanInterface::new(iface("vcan0"));
    let mut policy = ExponentialBackoff::default();
    let mut tp = TpEngine::new(TpTimers::default(), 8).with_max_etp_bytes(MAX_ETP);
    let t0 = Instant::now();

    // Drain bring-up events so we assert on the abort edge only.
    while sub.try_recv().is_ok() {}

    // ETP.RTS announcing 20000 bytes (> 8192 cap) from PEER_SA to OUR_SA.
    let oversize: u32 = 20_000;
    let pgn_v = pgn(0xFEEE & 0x3FFFF).value();
    inject_etp_cm(
        &driver,
        PEER_SA,
        OUR_SA,
        [
            ETP_RTS_CONTROL,
            (oversize & 0xFF) as u8,
            ((oversize >> 8) & 0xFF) as u8,
            ((oversize >> 16) & 0xFF) as u8,
            ((oversize >> 24) & 0xFF) as u8,
            (pgn_v & 0xFF) as u8,
            ((pgn_v >> 8) & 0xFF) as u8,
            ((pgn_v >> 16) & 0xFF) as u8,
        ],
    );

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

    assert_eq!(aborted, 1, "the oversize ETP.RTS was aborted");
    assert_eq!(
        abort_reason,
        Some(TpAbortReason::Resources),
        "oversize abort uses the Resources reason (system resources for another task)"
    );
    assert_eq!(
        tp.active_inbound_etp_sessions(),
        0,
        "NO reassembly buffer/session was allocated for the oversize announce"
    );

    // The abort surfaced as a HealthEvent (not a silent drop) — REQ_0894.
    let evt = sub
        .try_recv()
        .expect("a HealthEvent was emitted for the oversize ETP abort");
    assert_eq!(evt.to.kind(), ConnectorHealthKind::Degraded);
}

/// An ETP.Abort emitted onto the wire for the oversize case carries the
/// Resources reason on the ETP.CM PGN — verified at the engine level so
/// the on-wire abort is unambiguous.
#[test]
fn etp_oversize_emits_resources_abort_frame() {
    let mut rx = TpEngine::new(TpTimers::default(), 8).with_max_etp_bytes(8192);
    let now = Instant::now();
    let oversize: u32 = 50_000;
    let pgn_v = pgn(0xFEEE & 0x3FFFF).value();
    let data = [
        ETP_RTS_CONTROL,
        (oversize & 0xFF) as u8,
        ((oversize >> 8) & 0xFF) as u8,
        ((oversize >> 16) & 0xFF) as u8,
        ((oversize >> 24) & 0xFF) as u8,
        (pgn_v & 0xFF) as u8,
        ((pgn_v >> 8) & 0xFF) as u8,
        ((pgn_v >> 16) & 0xFF) as u8,
    ];
    let dec = decode_extended_id(encode_extended_id(
        pgn(ETP_CM_PGN),
        7,
        PEER_SA,
        Some(OUR_SA),
    ));
    let events = rx.on_frame(&dec, &data, now);
    assert_eq!(events.len(), 1);
    assert!(matches!(
        events[0],
        TpEvent::Aborted {
            reason: TpAbortReason::Resources,
            ..
        }
    ));
    assert_eq!(rx.active_inbound_etp_sessions(), 0, "no session allocated");
    let out = rx.take_outbound();
    assert_eq!(out.len(), 1, "an ETP.Abort frame is emitted on the wire");
    assert_eq!(out[0].data[0], ETP_ABORT_CONTROL);
    assert_eq!(out[0].data[1], TpAbortReason::Resources.as_u8());
    assert_eq!(
        out[0].wire_pgn.value(),
        ETP_CM_PGN,
        "abort rides the ETP.CM PGN"
    );
}
