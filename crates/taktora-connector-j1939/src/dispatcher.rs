//! Per-iface dispatcher tasks. `BB_0100`.
//!
//! Each owned CAN interface runs as one tokio task owning its
//! `CanInterfaceLike` driver instance (reused from
//! `taktora-connector-can`, `REQ_0899`). The task loop:
//!
//! 1. Drain outbound channels for single-frame PGNs — encode the
//!    29-bit id from the routing ([`crate::decode::encode_extended_id`])
//!    and `send_classical` via the driver.
//! 2. Await one inbound frame with a TX-tick timeout. On a data frame,
//!    decode the 29-bit id ([`crate::decode::decode_extended_id`]) and
//!    publish its payload to every inbound channel whose routing
//!    matches by PGN / SA / DA (`REQ_0890`). On an error frame,
//!    classify it into the health state machine.
//!
//! ## Transport-protocol seam (#123 / #124 / #125)
//!
//! This tracer bullet implements **single-frame** PGN routing only.
//! [`crate::routing::TransportClass::Tp`] channels validate their `N`
//! and register, but multi-packet send/receive is *not* wired here:
//!
//! * **RX:** `demux_inbound` skips channels whose `transport.is_tp()`
//!   — a single classical frame is never a complete TP payload. The TP
//!   reassembler (consuming TP.CM / TP.DT / ETP frames and emitting a
//!   reassembled payload) is inserted at the `demux_inbound` call
//!   site marked `TP-SEAM` below.
//! * **TX:** `collect_outbound_jobs` skips TP-class outbound channels.
//!   The TP segmenter (BAM broadcast / RTS-CTS handshake / ETP) is
//!   inserted at the marked `TP-SEAM` in that function.
//!
//! Issue #123 owns the BAM path, #124 RTS/CTS, #125 ETP. Each plugs
//! into these two seams plus a per-session state table that this module
//! will gain a field for.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use taktora_connector_can::{
    CanData, CanErrorKind, CanFdFlags, CanFrame, CanFrameKind, CanHealthMonitor, CanId, CanIface,
    CanIfaceState, CanInterfaceLike, CanIoError, ChannelBinding, Direction, IfaceHealthKind,
};
use taktora_connector_core::{ConnectorError, ReconnectPolicy};

use crate::addr_claim::{
    ADDRESS_CLAIMED_PGN, AcEvent, AcOutFrame, AddrClaimEngine, COMMANDED_ADDRESS_PGN, ClaimGate,
    ClaimState, REQUEST_PGN,
};
use crate::decode::{DecodedId, decode_extended_id, encode_extended_id};
use crate::registry::{J1939Registry, RegisteredChannel};
use crate::routing::{Pgn, SINGLE_FRAME_LEN};
use crate::tp::{TpEngine, TpEvent, TpOutFrame, TpTimers};

/// Default per-iteration TX drain tick.
pub const DEFAULT_TX_TICK: Duration = Duration::from_millis(1);

/// Per-iteration outcome surfaced from [`dispatch_one_iteration`].
#[derive(Debug, Default)]
pub struct IterationOutcome {
    /// Number of outbound frames sent on the driver this iteration.
    /// A segmented BAM message counts as its TP.CM + TP.DT frame total.
    pub tx_sent: usize,
    /// Number of inbound data frames demuxed to readers this iteration
    /// (each matching reader counted once). Includes reassembled TP
    /// payloads published to matching TP channels.
    pub inbound_publishes: usize,
    /// Error-frame discriminant observed, if any.
    pub error_kind: Option<CanErrorKind>,
    /// `true` when the inbound recv returned `Closed` and the
    /// dispatcher took the reconnect path.
    pub reconnected: bool,
    /// Number of TP sessions completed (reassembled) this iteration.
    pub tp_completed: usize,
    /// Number of TP sessions aborted (e.g. a T1 timeout) this iteration
    /// (`REQ_0895`).
    pub tp_aborted: usize,
    /// Number of inbound TP sessions refused at the concurrency cap this
    /// iteration (`REQ_0896`).
    pub tp_refused: usize,
    /// Every TP abort / refusal event raised this iteration, in order.
    /// Each one was surfaced as a `HealthEvent` (never a silent drop);
    /// tests inspect the carried [`crate::tp::TpAbortReason`].
    pub tp_events: Vec<TpEvent>,
}

/// Drive one full TX + RX iteration synchronously with a throwaway TP
/// engine. Backward-compatible entry point for single-frame test
/// harnesses (#122): it allocates a fresh [`TpEngine`] (so no TP session
/// state survives the call) and reads the clock once. Tests that exercise
/// multi-packet TP must use [`dispatch_one_iteration_tp`] to persist the
/// engine across iterations and control `now`.
///
/// `source_addr` is this node's J1939 source address on `iface`, used
/// as the TX source for outbound routings that leave `source_addr =
/// None`.
///
/// # Errors
///
/// Propagates driver / publish errors verbatim.
pub async fn dispatch_one_iteration<I>(
    iface: &CanIface,
    source_addr: u8,
    driver: &mut I,
    registry: &Arc<Mutex<J1939Registry>>,
    health: &Arc<CanHealthMonitor>,
    reconnect_policy: &mut dyn ReconnectPolicy,
    recv_timeout: Duration,
) -> Result<IterationOutcome, ConnectorError>
where
    I: CanInterfaceLike,
{
    let mut tp_engine = TpEngine::new(TpTimers::default(), crate::tp::DEFAULT_MAX_TP_SESSIONS);
    dispatch_one_iteration_tp(
        iface,
        source_addr,
        driver,
        registry,
        health,
        &mut tp_engine,
        reconnect_policy,
        recv_timeout,
        Instant::now(),
    )
    .await
}

/// Drive one full TX + RX iteration synchronously, stepping a
/// caller-owned [`TpEngine`] with an explicit `now` (`BB_0101`). This is
/// the TP-aware entry point: the engine persists multi-packet session
/// state across calls and `now` makes timer behaviour deterministic with
/// no real sleeps (`REQ_0895`).
///
/// Per iteration:
///
/// 1. **TX:** drain single-frame channels (as before) *and* segment any
///    `Tp`-class outbound channel into TP.CM(BAM) + TP.DT frames
///    (`REQ_0892`).
/// 2. **RX:** on a data frame, decode the id; TP.CM/TP.DT frames feed
///    [`TpEngine::on_frame`] (reassembly / session-bound enforcement),
///    other frames demux as single frames.
/// 3. **Timers:** call [`TpEngine::poll_timeouts`] with `now`.
///
/// Completed TP payloads publish to the matching channel; every TP abort
/// / refusal becomes a `HealthEvent` (`REQ_0895`/`REQ_0896`) and is also
/// recorded in the returned [`IterationOutcome`].
///
/// # Errors
///
/// Propagates driver / publish errors verbatim.
#[allow(clippy::too_many_arguments)]
pub async fn dispatch_one_iteration_tp<I>(
    iface: &CanIface,
    source_addr: u8,
    driver: &mut I,
    registry: &Arc<Mutex<J1939Registry>>,
    health: &Arc<CanHealthMonitor>,
    tp_engine: &mut TpEngine,
    reconnect_policy: &mut dyn ReconnectPolicy,
    recv_timeout: Duration,
    now: Instant,
) -> Result<IterationOutcome, ConnectorError>
where
    I: CanInterfaceLike,
{
    dispatch_iteration_inner(
        iface,
        source_addr,
        driver,
        registry,
        health,
        tp_engine,
        reconnect_policy,
        recv_timeout,
        now,
        None,
    )
    .await
}

/// Address-claim-aware iteration (`BB_0102`, `REQ_0897`/`REQ_0898`). Steps
/// a caller-owned [`AddrClaimEngine`] alongside the [`TpEngine`]: it ticks
/// the claim-wait timer, feeds inbound Address Claimed (PGN 60928) /
/// Request (PGN 59904) / Commanded-Address (PGN 65240) frames into the
/// claim engine, transmits the engine's outbound Address Claimed /
/// Cannot-Claim / request-response frames, and reflects every claim state
/// change onto `health` (Claiming → Connecting, Claimed → Up, CannotClaim
/// → Down) and the shared [`ClaimGate`] (the TX gate the
/// [`crate::J1939Writer`] reads).
///
/// This is the entry point [`dispatcher_loop`] uses and the seam
/// address-claim tests drive directly.
///
/// # Errors
///
/// Propagates driver / publish errors verbatim.
#[allow(clippy::too_many_arguments)]
pub async fn dispatch_one_iteration_claim<I>(
    iface: &CanIface,
    source_addr: u8,
    driver: &mut I,
    registry: &Arc<Mutex<J1939Registry>>,
    health: &Arc<CanHealthMonitor>,
    tp_engine: &mut TpEngine,
    claim_engine: &mut AddrClaimEngine,
    claim_gate: &ClaimGate,
    reconnect_policy: &mut dyn ReconnectPolicy,
    recv_timeout: Duration,
    now: Instant,
) -> Result<IterationOutcome, ConnectorError>
where
    I: CanInterfaceLike,
{
    dispatch_iteration_inner(
        iface,
        source_addr,
        driver,
        registry,
        health,
        tp_engine,
        reconnect_policy,
        recv_timeout,
        now,
        Some(ClaimCtx {
            engine: claim_engine,
            gate: claim_gate,
        }),
    )
    .await
}

/// Claim-engine context threaded through [`dispatch_iteration_inner`].
struct ClaimCtx<'a> {
    engine: &'a mut AddrClaimEngine,
    gate: &'a ClaimGate,
}

#[allow(clippy::too_many_arguments)]
async fn dispatch_iteration_inner<I>(
    iface: &CanIface,
    source_addr: u8,
    driver: &mut I,
    registry: &Arc<Mutex<J1939Registry>>,
    health: &Arc<CanHealthMonitor>,
    tp_engine: &mut TpEngine,
    reconnect_policy: &mut dyn ReconnectPolicy,
    recv_timeout: Duration,
    now: Instant,
    mut claim: Option<ClaimCtx<'_>>,
) -> Result<IterationOutcome, ConnectorError>
where
    I: CanInterfaceLike,
{
    // TX drain: single-frame PGNs plus BAM segmentation (TP-SEAM #123).
    let tx_sent = send_outbound_once(source_addr, driver, registry, tp_engine).await?;
    let mut outcome = IterationOutcome {
        tx_sent,
        ..Default::default()
    };

    let mut ac_events: Vec<AcEvent> = Vec::new();

    // Address-claim timer tick (BB_0102): first call emits the initial
    // Address Claimed; an uncontested claim transitions to Claimed once
    // the claim-wait elapses.
    if let Some(ctx) = claim.as_mut() {
        ac_events.extend(ctx.engine.poll(now));
    }

    // RX poll with timeout.
    let result: Option<Result<CanFrame, CanIoError>> = {
        let recv = driver.recv();
        tokio::pin!(recv);
        tokio::select! {
            biased;
            _ = tokio::time::sleep(recv_timeout) => None,
            res = &mut recv => Some(res),
        }
    };

    handle_rx_result(
        result,
        iface,
        driver,
        registry,
        health,
        tp_engine,
        reconnect_policy,
        claim.as_mut(),
        &mut ac_events,
        now,
        &mut outcome,
    )
    .await?;

    // TP timer sweep every iteration (TP-SEAM #123): T1 gaps abort
    // sessions and surface as HealthEvents (`REQ_0895`). Connection-mode
    // T2/T3 timeouts (#124) also queue a Conn_Abort frame, drained below.
    let timeouts = tp_engine.poll_timeouts(now);
    apply_tp_events(timeouts, registry, health, iface, &mut outcome)?;

    // TP-SEAM (#124 RTS/CTS): transmit any connection-mode frames the
    // engine produced this iteration in response to inbound frames or
    // timers (CTS / EndOfMsgAck / TP.DT bursts / Conn_Abort). BAM never
    // queues here, so this is a no-op on the #123 path.
    let out_frames = tp_engine.take_outbound();
    transmit_frames(driver, &out_frames, &mut outcome.tx_sent).await?;

    // Reflect address-claim state changes onto health + the TX gate, then
    // transmit any Address Claimed / Cannot-Claim / request-response
    // frames the claim engine queued (BB_0102, REQ_0898).
    if let Some(ctx) = claim.as_mut() {
        apply_ac_events(&ac_events, ctx.gate, health, iface);
        let claim_out = ctx.engine.take_outbound();
        transmit_frames(driver, &claim_out, &mut outcome.tx_sent).await?;
    }

    Ok(outcome)
}

/// Apply the RX poll result: demux a data frame, classify an error frame,
/// or drive a bus-off reconnect. Extracted from `dispatch_iteration_inner`
/// to keep that function within the complexity budget.
#[allow(clippy::too_many_arguments)]
async fn handle_rx_result<I>(
    result: Option<Result<CanFrame, CanIoError>>,
    iface: &CanIface,
    driver: &mut I,
    registry: &Arc<Mutex<J1939Registry>>,
    health: &Arc<CanHealthMonitor>,
    tp_engine: &mut TpEngine,
    reconnect_policy: &mut dyn ReconnectPolicy,
    claim: Option<&mut ClaimCtx<'_>>,
    ac_events: &mut Vec<AcEvent>,
    now: Instant,
    outcome: &mut IterationOutcome,
) -> Result<(), ConnectorError>
where
    I: CanInterfaceLike,
{
    match result {
        None | Some(Ok(CanFrame::Remote { .. })) => {}
        Some(Ok(CanFrame::Data(d))) => {
            let claim_engine = claim.map(|c| &mut *c.engine);
            demux_inbound(
                &d,
                registry,
                tp_engine,
                claim_engine,
                ac_events,
                health,
                iface,
                now,
                outcome,
            )?;
        }
        Some(Ok(CanFrame::Error(kind))) => {
            outcome.error_kind = Some(kind);
            classify_error(iface, kind, health, driver, reconnect_policy).await?;
        }
        Some(Err(CanIoError::Closed)) => {
            outcome.reconnected = true;
            reconnect_once(iface, driver, health, reconnect_policy).await;
        }
        Some(Err(e)) => return Err(ConnectorError::stack(IoErr(format!("recv: {e}")))),
    }
    Ok(())
}

/// Shared shape of the classical CAN frames the TP and address-claim
/// engines queue for transmission. Implemented for both `TpOutFrame` and
/// `AcOutFrame` so a single [`transmit_frames`] helper drains either.
trait OutFrame {
    fn parts(&self) -> (Pgn, u8, u8, Option<u8>, &[u8; 8]);
}

impl OutFrame for TpOutFrame {
    fn parts(&self) -> (Pgn, u8, u8, Option<u8>, &[u8; 8]) {
        (
            self.wire_pgn,
            self.priority,
            self.source_addr,
            self.dest_addr,
            &self.data,
        )
    }
}

impl OutFrame for AcOutFrame {
    fn parts(&self) -> (Pgn, u8, u8, Option<u8>, &[u8; 8]) {
        (
            self.wire_pgn,
            self.priority,
            self.source_addr,
            self.dest_addr,
            &self.data,
        )
    }
}

/// Encode + transmit each engine-produced frame as a classical CAN frame,
/// bumping `tx_sent` per send. Shared by the TP (`#124`) and address-claim
/// (`#126`) outbound paths.
async fn transmit_frames<I, F>(
    driver: &mut I,
    frames: &[F],
    tx_sent: &mut usize,
) -> Result<(), ConnectorError>
where
    I: CanInterfaceLike,
    F: OutFrame + Sync,
{
    for f in frames {
        let (wire_pgn, priority, source_addr, dest_addr, data) = f.parts();
        let raw_id = encode_extended_id(wire_pgn, priority, source_addr, dest_addr);
        let can_id = CanId::extended(raw_id)
            .map_err(|e| ConnectorError::stack(IoErr(format!("encode id: {e}"))))?;
        let frame = CanData::new(can_id, CanFrameKind::Classical, CanFdFlags::empty(), data)
            .map_err(|e| ConnectorError::stack(IoErr(format!("build frame: {e}"))))?;
        driver
            .send_classical(&frame)
            .await
            .map_err(|e| ConnectorError::stack(IoErr(format!("send_classical: {e}"))))?;
        *tx_sent += 1;
    }
    Ok(())
}

/// Reflect address-claim [`AcEvent`]s onto the shared [`ClaimGate`] (the
/// outbound TX gate) and the reused [`CanHealthMonitor`] (`REQ_0898`):
/// `Claiming → Connecting`, `Claimed → Up`, `CannotClaim → Down`.
///
/// Health transitions are best-effort: an illegal `ARCH_0012` edge (e.g.
/// `Up → Connecting` on a Commanded-Address re-claim) is swallowed so the
/// gate — which always tracks the true claim state — remains the
/// authoritative TX guard. The legal claim-driven edges
/// (`Connecting → Up`, `Connecting/Up → Down`) fire normally.
fn apply_ac_events(
    events: &[AcEvent],
    gate: &ClaimGate,
    health: &Arc<CanHealthMonitor>,
    iface: &CanIface,
) {
    for event in events {
        match event {
            AcEvent::Claiming { .. } => {
                gate.set(ClaimState::Claiming);
                let _ = health.set_iface(*iface, IfaceHealthKind::Connecting);
            }
            AcEvent::Claimed { .. } => {
                gate.set(ClaimState::Claimed);
                let _ = health.set_iface(*iface, IfaceHealthKind::Up);
            }
            AcEvent::CannotClaim => {
                gate.set(ClaimState::CannotClaim);
                let _ = health.set_iface(*iface, IfaceHealthKind::Down);
            }
        }
    }
}

/// Per-iface task entry point. Spawned by [`crate::J1939Connector`]'s
/// `register_with` for every configured interface.
///
/// Runs [`dispatch_one_iteration_claim`] in a loop until `stop` flips to
/// `true`. The interface does NOT start `Up`: it starts `Connecting`
/// (Claiming) and is driven `Up` by the address-claim engine once the
/// configured source address is claimed, or `Down` on cannot-claim
/// (`REQ_0898`). The shared `claim_gate` mirrors that state for the
/// [`crate::J1939Writer`] TX gate.
///
/// # Errors
///
/// Returns the first unrecoverable error from a dispatch iteration.
#[allow(clippy::too_many_arguments)]
pub async fn dispatcher_loop<I>(
    iface: CanIface,
    source_addr: u8,
    name: u64,
    claim_wait: Duration,
    claim_gate: Arc<ClaimGate>,
    mut driver: I,
    registry: Arc<Mutex<J1939Registry>>,
    health: Arc<CanHealthMonitor>,
    mut reconnect_policy: Box<dyn ReconnectPolicy>,
    stop: Arc<AtomicBool>,
    tx_tick: Duration,
    tp_timers: TpTimers,
    max_tp_sessions: usize,
    max_etp_bytes: usize,
) -> Result<(), ConnectorError>
where
    I: CanInterfaceLike,
{
    // One TP engine per interface dispatcher, persisted across iterations
    // so multi-packet sessions and their timers survive (`BB_0101`). The
    // ETP reassembly ceiling (`REQ_0894`/`REQ_0903`) is bound here.
    let mut tp_engine = TpEngine::new(tp_timers, max_tp_sessions).with_max_etp_bytes(max_etp_bytes);
    // One address-claim engine per interface (`BB_0102`). It starts in
    // Claiming; the first iteration emits the initial Address Claimed.
    let mut claim_engine = AddrClaimEngine::new(source_addr, name, claim_wait);
    while !stop.load(Ordering::Acquire) {
        dispatch_one_iteration_claim(
            &iface,
            source_addr,
            &mut driver,
            &registry,
            &health,
            &mut tp_engine,
            &mut claim_engine,
            &claim_gate,
            &mut *reconnect_policy,
            tx_tick,
            Instant::now(),
        )
        .await?;
    }
    Ok(())
}

/// Drain every single-frame outbound channel once, encode each
/// envelope into a classical CAN frame, and send it via the driver.
/// Returns the number of frames sent.
async fn send_outbound_once<I>(
    source_addr: u8,
    driver: &mut I,
    registry: &Mutex<J1939Registry>,
    tp_engine: &TpEngine,
) -> Result<usize, ConnectorError>
where
    I: CanInterfaceLike,
{
    let jobs = collect_outbound_jobs(source_addr, registry, tp_engine)?;
    let mut sent = 0usize;
    for data in &jobs {
        driver
            .send_classical(data)
            .await
            .map_err(|e| ConnectorError::stack(IoErr(format!("send_classical: {e}"))))?;
        sent += 1;
    }
    Ok(sent)
}

#[allow(clippy::significant_drop_tightening)]
fn collect_outbound_jobs(
    source_addr: u8,
    registry: &Mutex<J1939Registry>,
    tp_engine: &TpEngine,
) -> Result<Vec<CanData>, ConnectorError> {
    let guard = registry.lock().expect("registry mutex not poisoned");
    let mut jobs: Vec<CanData> = Vec::new();
    let mut buf = [0u8; SINGLE_FRAME_LEN];
    for entry in guard.iter_direction(Direction::Outbound) {
        let RegisteredChannel {
            routing, binding, ..
        } = entry;
        let ChannelBinding::Outbound(drain) = binding else {
            continue;
        };
        let sa = routing.source_addr.unwrap_or(source_addr);

        // TP-SEAM (#123 BAM / #124 RTS-CTS / #125 ETP): drain the whole
        // message and segment it into TP.CM(BAM) + TP.DT frames.
        if routing.transport.is_tp() {
            let mut tp_buf = vec![0u8; routing.transport.max_payload()];
            while let Some(written) = drain.drain_into(&mut tp_buf)? {
                let frames = tp_engine
                    .segment_outbound(routing.pgn, routing.priority, sa, &tp_buf[..written])
                    .map_err(|e| ConnectorError::stack(IoErr(format!("segment_outbound: {e}"))))?;
                for f in frames {
                    let raw_id =
                        encode_extended_id(f.wire_pgn, f.priority, f.source_addr, f.dest_addr);
                    let can_id = CanId::extended(raw_id)
                        .map_err(|e| ConnectorError::stack(IoErr(format!("encode id: {e}"))))?;
                    let data = CanData::new(
                        can_id,
                        CanFrameKind::Classical,
                        CanFdFlags::empty(),
                        &f.data,
                    )
                    .map_err(|e| ConnectorError::stack(IoErr(format!("build frame: {e}"))))?;
                    jobs.push(data);
                }
            }
            continue;
        }

        let raw_id = encode_extended_id(routing.pgn, routing.priority, sa, routing.dest_addr);
        let can_id = CanId::extended(raw_id)
            .map_err(|e| ConnectorError::stack(IoErr(format!("encode id: {e}"))))?;
        loop {
            let Some(written) = drain.drain_into(&mut buf)? else {
                break;
            };
            let data = CanData::new(
                can_id,
                CanFrameKind::Classical,
                CanFdFlags::empty(),
                &buf[..written],
            )
            .map_err(|e| ConnectorError::stack(IoErr(format!("build frame: {e}"))))?;
            jobs.push(data);
        }
    }
    Ok(jobs)
}

/// Decode `frame`'s id and route it. TP.CM/TP.DT frames feed the TP
/// engine (reassembly + session-bound enforcement); everything else
/// demuxes to matching single-frame inbound channels. Updates `outcome`
/// in place (`inbound_publishes`, plus TP completion/abort/refusal
/// counters and events).
#[allow(clippy::too_many_arguments)]
fn demux_inbound(
    frame: &CanData,
    registry: &Arc<Mutex<J1939Registry>>,
    tp_engine: &mut TpEngine,
    claim_engine: Option<&mut AddrClaimEngine>,
    ac_events: &mut Vec<AcEvent>,
    health: &Arc<CanHealthMonitor>,
    iface: &CanIface,
    now: Instant,
    outcome: &mut IterationOutcome,
) -> Result<(), ConnectorError> {
    let decoded = decode_extended_id(frame.id.value);

    // Address-claim seam (#126): Address Claimed / Request / Commanded
    // Address frames feed the claim state machine rather than the readers —
    // but ONLY when a claim engine is active. With no claim engine (the
    // backward-compatible single-frame / TP entry points) these PGNs route
    // normally so #122–#125 behaviour is unchanged.
    if is_claim_pgn(&decoded) {
        if let Some(engine) = claim_engine {
            ac_events.extend(engine.on_frame(&decoded, frame.payload(), now));
            return Ok(());
        }
    }

    // TP-SEAM (#123/#124/#125): TP.CM / TP.DT frames are consumed by the
    // session engine rather than published as single frames.
    if is_tp_wire_pgn(&decoded) {
        let events = tp_engine.on_frame(&decoded, frame.payload(), now);
        apply_tp_events(events, registry, health, iface, outcome)?;
        return Ok(());
    }

    let guard = registry.lock().expect("registry mutex not poisoned");
    for entry in guard.iter_direction(Direction::Inbound) {
        let RegisteredChannel {
            routing, binding, ..
        } = entry;
        // A single classical frame is never a complete TP payload; TP
        // channels are fed by the reassembler above.
        if routing.transport.is_tp() {
            continue;
        }
        if !routing.matches(&decoded) {
            continue;
        }
        if let ChannelBinding::Inbound(publish) = binding {
            publish.publish_bytes(frame.payload())?;
            outcome.inbound_publishes += 1;
        }
    }
    Ok(())
}

/// `true` when a decoded id carries a J1939-81 address-claim PGN — Address
/// Claimed (60928), Request (59904), or Commanded Address (65240). These
/// feed the [`AddrClaimEngine`], never the channel readers (#126).
fn is_claim_pgn(decoded: &DecodedId) -> bool {
    matches!(
        decoded.pgn.value(),
        ADDRESS_CLAIMED_PGN | REQUEST_PGN | COMMANDED_ADDRESS_PGN
    )
}

/// `true` when a decoded id carries a TP/ETP wire PGN (TP.CM / TP.DT /
/// ETP.CM / ETP.DT). These are consumed by the session engine, never
/// published as single frames (#123/#124/#125).
fn is_tp_wire_pgn(decoded: &DecodedId) -> bool {
    matches!(
        decoded.pgn.value(),
        crate::tp::TP_CM_PGN | crate::tp::TP_DT_PGN | crate::tp::ETP_CM_PGN | crate::tp::ETP_DT_PGN
    )
}

/// Translate TP engine events into side effects: publish completed
/// payloads to the matching channel, and turn aborts / refusals into
/// `HealthEvent`s (`REQ_0895`/`REQ_0896`) — never a silent drop.
fn apply_tp_events(
    events: Vec<TpEvent>,
    registry: &Arc<Mutex<J1939Registry>>,
    health: &Arc<CanHealthMonitor>,
    iface: &CanIface,
    outcome: &mut IterationOutcome,
) -> Result<(), ConnectorError> {
    for event in events {
        match event {
            TpEvent::Completed {
                pgn,
                source_addr,
                payload,
            } => {
                outcome.inbound_publishes +=
                    publish_tp_completed(registry, pgn, source_addr, &payload)?;
                outcome.tp_completed += 1;
            }
            TpEvent::Aborted { reason, .. } => {
                degrade_health(health, iface, &format!("TP session aborted: {reason}"));
                outcome.tp_aborted += 1;
                outcome.tp_events.push(event);
            }
            TpEvent::SessionRefused { reason, .. } => {
                degrade_health(health, iface, &format!("TP session refused: {reason}"));
                outcome.tp_refused += 1;
                outcome.tp_events.push(event);
            }
        }
    }
    Ok(())
}

/// Surface a TP abort / refusal as a `HealthEvent`. A BAM session is
/// connectionless, so this is purely a local health transition (no
/// TP.Conn_Abort on the wire — that is #124's connection-mode job). The
/// `Up`/`Connecting → Degraded` edge is legal per `ARCH_0012`; an
/// already-degraded aggregate yields no new event (which is fine — the
/// degraded condition is already observable).
fn degrade_health(health: &Arc<CanHealthMonitor>, iface: &CanIface, _reason: &str) {
    let _ = health.set_iface(*iface, IfaceHealthKind::Degraded);
}

/// Publish a reassembled TP payload to every matching TP inbound channel.
/// BAM has no destination address, so the synthesized id carries
/// `dest_addr = None` (a channel with a DA filter therefore won't match,
/// matching single-frame broadcast semantics). Returns the publish count.
#[allow(clippy::significant_drop_tightening)]
fn publish_tp_completed(
    registry: &Mutex<J1939Registry>,
    pgn: Pgn,
    source_addr: u8,
    payload: &[u8],
) -> Result<usize, ConnectorError> {
    let synthetic = DecodedId {
        priority: 0,
        pdu_format: pgn.pdu_format(),
        pgn,
        source_addr,
        dest_addr: None,
    };
    let guard = registry.lock().expect("registry mutex not poisoned");
    let mut count = 0usize;
    for entry in guard.iter_direction(Direction::Inbound) {
        let RegisteredChannel {
            routing, binding, ..
        } = entry;
        if !routing.transport.is_tp() || !routing.matches(&synthetic) {
            continue;
        }
        if let ChannelBinding::Inbound(publish) = binding {
            publish.publish_bytes(payload)?;
            count += 1;
        }
    }
    Ok(count)
}

async fn classify_error<I>(
    iface: &CanIface,
    kind: CanErrorKind,
    health: &Arc<CanHealthMonitor>,
    driver: &mut I,
    reconnect_policy: &mut dyn ReconnectPolicy,
) -> Result<(), ConnectorError>
where
    I: CanInterfaceLike,
{
    match kind {
        CanErrorKind::Warning | CanErrorKind::Passive => {
            let _ = health.set_iface(*iface, IfaceHealthKind::Degraded);
            Ok(())
        }
        CanErrorKind::BusOff => {
            let _ = health.set_iface(*iface, IfaceHealthKind::Down);
            let delay = reconnect_policy.next_delay();
            tokio::time::sleep(delay).await;
            match driver.reopen().await {
                Ok(()) => {
                    let _ = health.set_iface(*iface, IfaceHealthKind::Connecting);
                    let _ = health.set_iface(*iface, IfaceHealthKind::Up);
                    Ok(())
                }
                Err(e) => Err(ConnectorError::stack(IoErr(format!("reopen: {e}")))),
            }
        }
        CanErrorKind::ArbitrationLost | CanErrorKind::Other => Ok(()),
    }
}

async fn reconnect_once<I>(
    iface: &CanIface,
    driver: &mut I,
    health: &Arc<CanHealthMonitor>,
    reconnect_policy: &mut dyn ReconnectPolicy,
) where
    I: CanInterfaceLike,
{
    let _ = health.set_iface(*iface, IfaceHealthKind::Down);
    let delay = reconnect_policy.next_delay();
    tokio::time::sleep(delay).await;
    if driver.reopen().await.is_ok() && matches!(driver.state(), CanIfaceState::Active) {
        let _ = health.set_iface(*iface, IfaceHealthKind::Connecting);
        let _ = health.set_iface(*iface, IfaceHealthKind::Up);
    }
}

#[derive(Debug)]
struct IoErr(String);

impl core::fmt::Display for IoErr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "j1939 dispatcher: {}", self.0)
    }
}

impl std::error::Error for IoErr {}
