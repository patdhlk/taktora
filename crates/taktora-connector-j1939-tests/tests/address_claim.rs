//! Layer-1 acceptance coverage for the J1939-81 address-claim state
//! machine (issue #126 / FEAT_0098). `REQ_0897`, `REQ_0898`, `BB_0102`,
//! `ADR_0110`.
//!
//! * TEST_0893 — address-claim contention & cannot-claim: a competing
//!   Address Claimed for the SAME source address carrying a HIGHER-priority
//!   (numerically LOWER) NAME makes the connector cede to the null address
//!   254 (CannotClaim); it still responds to a Request for Address Claimed
//!   and honours an Address-Commanded message.
//! * TEST_0894 — claim state drives health & gates TX: the interface
//!   reports `Connecting` while Claiming, `Up` once Claimed, and `Down` on
//!   CannotClaim (observed through the reused `CanHealthMonitor` /
//!   subscription); and the gated `J1939Writer::send` returns
//!   `ConnectorError::Down` until the address is Claimed, then succeeds.
//!
//! ## Test seams
//!
//! The address-claim engine is clock-stepped (explicit `now: Instant`)
//! like the TP engine, so timers advance with no real sleeps. The
//! contention / request / commanded-address logic is exercised directly on
//! the public `AddrClaimEngine` (TEST_0893). Health transitions are driven
//! through `dispatch_one_iteration_claim` over the single-queue
//! `MockJ1939Interface` loopback, and the TX gate is asserted on the
//! `J1939Writer` the connector hands back (TEST_0894) — the framework
//! `ChannelWriter` is a shared transport type, so the gate lives on the
//! J1939 wrapper.

#![allow(clippy::doc_markdown)]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use taktora_connector_can::{CanHealthMonitor, CanIface, MockCanInterface};
use taktora_connector_codec::JsonCodec;
use taktora_connector_core::{
    ChannelDescriptor, ConnectorError, ConnectorHealthKind, ExponentialBackoff,
};
use taktora_connector_j1939::{
    ADDRESS_CLAIM_PRIORITY, ADDRESS_CLAIMED_PGN, AcEvent, AddrClaimEngine, COMMANDED_ADDRESS_PGN,
    ClaimState, DEFAULT_CLAIM_WAIT, J1939Connector, J1939ConnectorOptions, J1939Interface,
    J1939Registry, J1939Routing, J1939State, MockJ1939Interface, NULL_ADDRESS, Pgn,
    REQUEST_FOR_ADDRESS_CLAIMED, REQUEST_PGN, TpEngine, TpTimers, TransportClass,
    dispatch_one_iteration_claim,
};

const SA: u8 = 0x80;
const OUR_NAME: u64 = 0x0000_0000_00AB_CDEF;
const RECV_TICK: Duration = Duration::from_millis(5);

fn iface(name: &str) -> CanIface {
    CanIface::new(name).unwrap()
}

fn pgn(v: u32) -> Pgn {
    Pgn::new(v).unwrap()
}

// ---------------------------------------------------------------------------
// TEST_0893 — contention & cannot-claim, request response, commanded address
// ---------------------------------------------------------------------------

#[test]
fn test_0893_contention_cedes_to_null_then_serves_request_and_command() {
    let t0 = Instant::now();
    let mut eng = AddrClaimEngine::new(SA, OUR_NAME, DEFAULT_CLAIM_WAIT);

    // Enter Claiming and emit the initial Address Claimed for our SA.
    let ev = eng.poll(t0);
    assert_eq!(ev, vec![AcEvent::Claiming { source_addr: SA }]);
    let out = eng.take_outbound();
    assert_eq!(out.len(), 1, "initial Address Claimed emitted");
    assert_eq!(out[0].wire_pgn.value(), ADDRESS_CLAIMED_PGN);
    assert_eq!(out[0].source_addr, SA);
    assert_eq!(u64::from_le_bytes(out[0].data), OUR_NAME);

    // A competing Address Claimed for the SAME SA with a HIGHER-priority
    // (numerically LOWER) NAME → we lose arbitration and cede to null 254.
    let competitor_name = OUR_NAME - 1;
    let competing = decoded_ac(SA);
    let ev = eng.on_frame(&competing, &competitor_name.to_le_bytes(), t0);
    assert_eq!(ev, vec![AcEvent::CannotClaim]);
    assert_eq!(eng.state(), ClaimState::CannotClaim);
    assert_eq!(eng.source_addr(), NULL_ADDRESS, "ceded to null address 254");

    // The Cannot-Claim message is an Address Claimed from the null address.
    let out = eng.take_outbound();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].source_addr, NULL_ADDRESS);
    assert_eq!(u64::from_le_bytes(out[0].data), OUR_NAME);

    // Respond to a Request for Address Claimed (PGN 59904) — while in
    // CannotClaim we answer from the null address.
    let req = decoded_request(0x21);
    let ev = eng.on_frame(&req, &REQUEST_FOR_ADDRESS_CLAIMED, t0);
    assert!(ev.is_empty(), "a request produces no state change");
    let out = eng.take_outbound();
    assert_eq!(out.len(), 1, "request answered");
    assert_eq!(out[0].wire_pgn.value(), ADDRESS_CLAIMED_PGN);
    assert_eq!(out[0].source_addr, NULL_ADDRESS);

    // Honour an Address-Commanded message (PGN 65240) addressed to our
    // NAME: adopt the commanded SA and re-enter Claiming. The 9-byte
    // reassembled payload is fed directly (the BAM/TP path delivers the
    // same bytes on the wire).
    let new_sa = 0x42u8;
    let mut command = OUR_NAME.to_le_bytes().to_vec();
    command.push(new_sa);
    let cmd = decoded_command();
    let ev = eng.on_frame(&cmd, &command, t0);
    assert_eq!(
        ev,
        vec![AcEvent::Claiming {
            source_addr: new_sa
        }]
    );
    assert_eq!(eng.state(), ClaimState::Claiming);
    assert_eq!(eng.source_addr(), new_sa, "adopted the commanded address");
    let out = eng.take_outbound();
    assert_eq!(out.len(), 1, "re-asserts Address Claimed for the new SA");
    assert_eq!(out[0].source_addr, new_sa);
}

#[test]
fn test_0893_higher_competitor_name_loses_and_we_keep_the_address() {
    let t0 = Instant::now();
    let mut eng = AddrClaimEngine::new(SA, OUR_NAME, DEFAULT_CLAIM_WAIT);
    let _ = eng.poll(t0);
    let _ = eng.take_outbound();

    // Competing claim with a LOWER-priority (numerically HIGHER) NAME → we
    // win and re-assert; no cede.
    let ev = eng.on_frame(&decoded_ac(SA), &(OUR_NAME + 1).to_le_bytes(), t0);
    assert!(ev.is_empty());
    assert_eq!(eng.state(), ClaimState::Claiming);
    assert_eq!(eng.source_addr(), SA);
    let out = eng.take_outbound();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].source_addr, SA, "re-asserted our claim");
}

// ---------------------------------------------------------------------------
// TEST_0894 — claim state drives health (through the dispatcher)
// ---------------------------------------------------------------------------

#[test]
fn test_0894_claim_state_drives_health_through_dispatcher() {
    let ifc = iface("vcan0");
    let claim_wait = Duration::from_millis(100);

    let registry = Arc::new(Mutex::new(J1939Registry::with_capacity(8)));
    let health = Arc::new(CanHealthMonitor::new(&[ifc]));
    let mut tp_engine = TpEngine::new(TpTimers::default(), 8);
    let mut claim_engine = AddrClaimEngine::new(SA, OUR_NAME, claim_wait);
    let gate = taktora_connector_j1939::ClaimGate::new();
    let mut policy = ExponentialBackoff::default();

    let mut harness = MockJ1939Interface::new(ifc);
    let sub = health.subscribe();

    // Fresh monitor: Connecting (Claiming). Gate closed.
    assert_eq!(health.current().kind(), ConnectorHealthKind::Connecting);

    let t0 = Instant::now();

    // Iteration 1 (t0): emit the initial Address Claimed; still Claiming.
    step(
        &ifc,
        &registry,
        &health,
        &mut tp_engine,
        &mut claim_engine,
        &gate,
        &mut policy,
        &mut harness,
        t0,
    );
    assert_eq!(health.current().kind(), ConnectorHealthKind::Connecting);
    assert!(!gate.is_claimed(), "gate closed while Claiming");

    // Iteration 2 (t0 + claim_wait): the claim-wait elapses uncontested →
    // Claimed → health Up.
    step(
        &ifc,
        &registry,
        &health,
        &mut tp_engine,
        &mut claim_engine,
        &gate,
        &mut policy,
        &mut harness,
        t0 + claim_wait,
    );
    assert_eq!(claim_engine.state(), ClaimState::Claimed);
    assert_eq!(health.current().kind(), ConnectorHealthKind::Up);
    assert!(gate.is_claimed(), "gate open once Claimed");

    // Inject a competing Address Claimed for the SAME SA with a higher-
    // priority (lower) NAME. Drive iterations until it is consumed and we
    // cede to CannotClaim → health Down.
    harness
        .inject_j1939(
            pgn(ADDRESS_CLAIMED_PGN),
            ADDRESS_CLAIM_PRIORITY,
            SA,
            Some(0xFF),
            &(OUR_NAME - 1).to_le_bytes(),
        )
        .unwrap();

    let mut down = false;
    for _ in 0..16 {
        step(
            &ifc,
            &registry,
            &health,
            &mut tp_engine,
            &mut claim_engine,
            &gate,
            &mut policy,
            &mut harness,
            t0 + claim_wait,
        );
        if claim_engine.state() == ClaimState::CannotClaim {
            down = true;
            break;
        }
    }
    assert!(down, "ceded to CannotClaim after losing arbitration");
    assert_eq!(claim_engine.source_addr(), NULL_ADDRESS);
    assert_eq!(health.current().kind(), ConnectorHealthKind::Down);
    assert!(!gate.is_claimed(), "gate closed on CannotClaim");

    // The subscription observed the Connecting → Up → Down arc.
    let kinds: Vec<ConnectorHealthKind> = std::iter::from_fn(|| sub.try_recv().ok())
        .map(|e| e.to.kind())
        .collect();
    assert!(
        kinds.contains(&ConnectorHealthKind::Up) && kinds.contains(&ConnectorHealthKind::Down),
        "health subscription saw Up then Down, got {kinds:?}"
    );
}

// ---------------------------------------------------------------------------
// TEST_0894 — the gated writer's send is gated until Claimed
// ---------------------------------------------------------------------------

#[test]
fn test_0894_gated_writer_send_blocks_until_claimed() {
    let ifc = iface("vcan0");
    let opts = J1939ConnectorOptions::builder()
        .interface(J1939Interface::new(ifc, SA).with_name(OUR_NAME))
        .build();
    let state = Arc::new(J1939State::new(opts));
    let driver = MockCanInterface::new(ifc);
    let connector = J1939Connector::<MockCanInterface, JsonCodec>::new(
        Arc::clone(&state),
        vec![driver],
        JsonCodec::new(),
    )
    .expect("construct J1939Connector");

    let routing = J1939Routing {
        pgn: pgn(0xFE00),
        source_addr: None,
        dest_addr: None,
        transport: TransportClass::SingleFrame,
        priority: 6,
    };
    let desc = ChannelDescriptor::<J1939Routing, 8>::new("addr_claim.gated", routing).unwrap();
    let writer = connector
        .create_gated_writer::<u32, 8>(&desc, &ifc)
        .expect("gated writer");

    // While Claiming (the default gate state) send is gated → Down.
    let err = writer.send(&42u32).expect_err("send gated before claim");
    assert!(matches!(err, ConnectorError::Down { .. }), "got {err:?}");

    // Drive the shared gate to Claimed (as the dispatcher would on a
    // successful claim) → send now delegates to the inner ChannelWriter.
    state.claim_gate(&ifc).unwrap().set(ClaimState::Claimed);
    writer.send(&42u32).expect("send succeeds once Claimed");

    // Cannot-claim re-gates outbound transmission.
    state.claim_gate(&ifc).unwrap().set(ClaimState::CannotClaim);
    let err = writer
        .send(&42u32)
        .expect_err("send re-gated on CannotClaim");
    assert!(matches!(err, ConnectorError::Down { .. }), "got {err:?}");
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn decoded_ac(sa: u8) -> taktora_connector_j1939::DecodedId {
    decode(ADDRESS_CLAIMED_PGN, sa)
}

fn decoded_request(sa: u8) -> taktora_connector_j1939::DecodedId {
    decode(REQUEST_PGN, sa)
}

fn decoded_command() -> taktora_connector_j1939::DecodedId {
    decode(COMMANDED_ADDRESS_PGN, 0x00)
}

fn decode(pgn_value: u32, sa: u8) -> taktora_connector_j1939::DecodedId {
    taktora_connector_j1939::decode_extended_id(taktora_connector_j1939::encode_extended_id(
        pgn(pgn_value),
        ADDRESS_CLAIM_PRIORITY,
        sa,
        Some(0xFF),
    ))
}

#[allow(clippy::too_many_arguments)]
fn step(
    ifc: &CanIface,
    registry: &Arc<Mutex<J1939Registry>>,
    health: &Arc<CanHealthMonitor>,
    tp_engine: &mut TpEngine,
    claim_engine: &mut AddrClaimEngine,
    gate: &taktora_connector_j1939::ClaimGate,
    policy: &mut ExponentialBackoff,
    harness: &mut MockJ1939Interface,
    now: Instant,
) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    rt.block_on(async {
        dispatch_one_iteration_claim(
            ifc,
            SA,
            harness.driver_mut(),
            registry,
            health,
            tp_engine,
            claim_engine,
            gate,
            policy,
            RECV_TICK,
            now,
        )
        .await
        .expect("dispatch iteration");
    });
}
