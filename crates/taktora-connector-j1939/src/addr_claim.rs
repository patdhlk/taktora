//! Full J1939-81 address-claim state machine. `BB_0102`, `REQ_0897`,
//! `REQ_0898`, `ADR_0110`.
//!
//! This is the last building block of the J1939 connector cluster. It is
//! modelled exactly like the [`crate::tp::TpEngine`]: a clock-stepped,
//! I/O-free state machine the dispatcher drives with an explicit
//! `now: Instant` so its claim-wait timer is testable with no real
//! sleeps. The dispatcher feeds it decoded inbound frames
//! ([`AddrClaimEngine::on_frame`]), ticks it every iteration
//! ([`AddrClaimEngine::poll`]), and transmits the outbound frames it
//! queues ([`AddrClaimEngine::take_outbound`]).
//!
//! ## J1939-81 protocol facts implemented here
//!
//! * **Address Claimed** (PGN [`ADDRESS_CLAIMED_PGN`] = 60928 / 0xEE00):
//!   broadcast (destination = global 0xFF) from source address `SA`,
//!   data = the 8-byte **NAME**, little-endian (NAME byte0 = LSB). A node
//!   claims `SA` by sending this.
//! * **NAME arbitration**: the numerically **lower** NAME wins (higher
//!   priority). On contention for the same SA, the contender with the
//!   lower NAME keeps/gets the address; the loser cedes.
//! * **Cannot Claim**: a node that loses arbitration sends an Address
//!   Claimed from the **null address** ([`NULL_ADDRESS`] = 254 / 0xFE)
//!   carrying its NAME. The engine then sits in [`ClaimState::CannotClaim`].
//! * **Request for Address Claimed**: the Request PGN
//!   [`REQUEST_PGN`] = 59904 / 0xEA00, whose 3 data bytes are the
//!   requested PGN little-endian = PGN 60928 →
//!   [`REQUEST_FOR_ADDRESS_CLAIMED`] `[0x00, 0xEE, 0x00]`. On receipt the
//!   engine (re)sends its current Address Claimed (or the Cannot-Claim
//!   null message when in [`ClaimState::CannotClaim`]).
//! * **Commanded Address** (PGN [`COMMANDED_ADDRESS_PGN`] = 65240 /
//!   0xFED8): a 9-byte payload = 8-byte target NAME (LE) + 1 byte new
//!   source address. Because it is 9 bytes (> 8) it crosses the wire as a
//!   BAM/TP message; the engine consumes the **reassembled** 9-byte
//!   payload (see the module note on the chosen path). When the commanded
//!   NAME equals this engine's NAME, the engine adopts the new SA and
//!   re-enters [`ClaimState::Claiming`].
//!
//! ## Commanded-Address delivery path
//!
//! Commanded Address is a 9-byte (> 8) message, so on a real bus it is a
//! BAM/TP transfer (PGN 65240). [`AddrClaimEngine::on_frame`] accepts a
//! decoded frame whose PGN is [`COMMANDED_ADDRESS_PGN`] carrying the
//! **already-reassembled** 9-byte payload. The dispatcher therefore feeds
//! the engine either (a) a single-frame 65240 (the deterministic
//! single-queue test path) or (b) the BAM-reassembled payload from the TP
//! engine. Tests drive path (a) directly via the engine, which is the
//! cleanest seam.
//!
//! ## State / health / TX-gating
//!
//! The three claim states map onto connector health (`REQ_0898`):
//! `Claiming → Connecting`, `Claimed → Up`, `CannotClaim → Down`. The
//! dispatcher reflects [`AcEvent`]s onto the reused
//! [`taktora_connector_can::CanHealthMonitor`] and onto a shared
//! [`ClaimGate`] that the [`crate::J1939Writer`] consults to gate
//! outbound transmission until `Claimed`.

use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};

use crate::decode::{DecodedId, GLOBAL_ADDRESS};
use crate::routing::Pgn;

/// Address Claimed PGN — `0xEE00` = 60928. PDU1 by format but broadcast
/// (destination = global `0xFF`). Data = the 8-byte NAME, little-endian.
pub const ADDRESS_CLAIMED_PGN: u32 = 60928;

/// Request PGN — `0xEA00` = 59904. PDU1. A Request for Address Claimed
/// carries the 3 little-endian bytes of PGN 60928 as its data.
pub const REQUEST_PGN: u32 = 59904;

/// Commanded Address PGN — `0xFED8` = 65240. PDU2 broadcast. Payload is 9
/// bytes (target NAME LE + new source address), so it crosses the wire as
/// a BAM/TP message.
pub const COMMANDED_ADDRESS_PGN: u32 = 65240;

/// J1939 null (unclaimed) source address — 254 / `0xFE`. A node that
/// cannot claim an address sends its Address Claimed from here.
pub const NULL_ADDRESS: u8 = 0xFE;

/// Standard J1939 priority for Address Claimed / Cannot Claim frames.
pub const ADDRESS_CLAIM_PRIORITY: u8 = 6;

/// Default claim-wait timer (J1939-81): the time an uncontested claim
/// waits before it is considered `Claimed`. Configurable via
/// [`crate::options::J1939ConnectorOptions`].
pub const DEFAULT_CLAIM_WAIT: Duration = Duration::from_millis(250);

/// The 3-byte data payload of a Request for Address Claimed: PGN 60928
/// little-endian (`0xEE00` → `[0x00, 0xEE, 0x00]`).
pub const REQUEST_FOR_ADDRESS_CLAIMED: [u8; 3] = [0x00, 0xEE, 0x00];

/// Address-claim state machine state (`REQ_0897`).
///
/// Maps onto connector health (`REQ_0898`): `Claiming → Connecting`,
/// `Claimed → Up`, `CannotClaim → Down`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ClaimState {
    /// The configured source address is being claimed; the claim-wait
    /// timer is running. Outbound transmission is gated (`REQ_0898`).
    Claiming = 0,
    /// The source address is owned; outbound transmission is permitted.
    Claimed = 1,
    /// Arbitration was lost (a higher-priority — lower — NAME contended
    /// for the same SA); the engine ceded to the null address.
    CannotClaim = 2,
}

impl ClaimState {
    const fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Claimed,
            2 => Self::CannotClaim,
            _ => Self::Claiming,
        }
    }
}

/// A state change produced by the [`AddrClaimEngine`] for the dispatcher
/// to reflect onto health and the [`ClaimGate`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AcEvent {
    /// Entered (or re-entered) `Claiming` for `source_addr` → health
    /// `Connecting`, gate closed.
    Claiming {
        /// The source address now being claimed.
        source_addr: u8,
    },
    /// The claim succeeded for `source_addr` → health `Up`, gate open.
    Claimed {
        /// The source address now owned.
        source_addr: u8,
    },
    /// Arbitration lost; ceded to the null address → health `Down`, gate
    /// closed.
    CannotClaim,
}

/// One ready-to-encode outbound address-claim frame, analogous to
/// [`crate::tp::TpOutFrame`]. The dispatcher encodes the 29-bit id from
/// `(wire_pgn, priority, source_addr, dest_addr)` and sends `data` as a
/// single classical CAN frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AcOutFrame {
    /// Wire PGN — always [`ADDRESS_CLAIMED_PGN`] (an Address Claimed or
    /// Cannot-Claim message).
    pub wire_pgn: Pgn,
    /// TX priority for the encoded identifier.
    pub priority: u8,
    /// Source address — the claimed SA, or [`NULL_ADDRESS`] for a
    /// Cannot-Claim message.
    pub source_addr: u8,
    /// Destination address — always `Some(0xFF)` (global broadcast).
    pub dest_addr: Option<u8>,
    /// The 8 NAME bytes, little-endian (NAME byte0 = LSB).
    pub data: [u8; 8],
}

/// Shared, lock-free claim-state handle bridging the per-interface
/// dispatcher (which owns the [`AddrClaimEngine`] and updates this gate on
/// each state change) and the application-facing [`crate::J1939Writer`]
/// (which reads it to gate outbound transmission). `REQ_0898`.
///
/// The framework's `ChannelWriter` is a shared transport type that cannot
/// carry connector-specific claim state, so the gate lives here in the
/// J1939 crate and the wrapper holds an `Arc<ClaimGate>`.
#[derive(Debug)]
pub struct ClaimGate {
    state: AtomicU8,
}

impl ClaimGate {
    /// Construct a gate in the initial [`ClaimState::Claiming`] state
    /// (transmission gated until a claim succeeds).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: AtomicU8::new(ClaimState::Claiming as u8),
        }
    }

    /// Snapshot the current claim state.
    #[must_use]
    pub fn state(&self) -> ClaimState {
        ClaimState::from_u8(self.state.load(Ordering::Acquire))
    }

    /// Publish a new claim state (called by the dispatcher).
    pub fn set(&self, state: ClaimState) {
        self.state.store(state as u8, Ordering::Release);
    }

    /// `true` only when the address is `Claimed` — i.e. outbound
    /// transmission is permitted.
    #[must_use]
    pub fn is_claimed(&self) -> bool {
        matches!(self.state(), ClaimState::Claimed)
    }
}

impl Default for ClaimGate {
    fn default() -> Self {
        Self::new()
    }
}

/// Per-interface J1939-81 address-claim state machine (`REQ_0897`).
///
/// Clock-stepped and I/O-free, mirroring [`crate::tp::TpEngine`]. Holds no
/// health monitor, no clock, and no driver handle — the dispatcher steps
/// it and owns the side effects.
#[derive(Debug)]
pub struct AddrClaimEngine {
    name: u64,
    /// The address we currently hold or seek; becomes [`NULL_ADDRESS`] on
    /// cannot-claim.
    current_sa: u8,
    state: ClaimState,
    claim_wait: Duration,
    /// When the current `Claiming` window started; `None` means the
    /// initial Address Claimed has not been emitted yet (the next
    /// [`Self::poll`] emits it and starts the timer).
    started_at: Option<Instant>,
    priority: u8,
    out: Vec<AcOutFrame>,
}

impl AddrClaimEngine {
    /// Construct an engine that will claim `configured_sa` using the
    /// 64-bit `name`, considering the claim successful after `claim_wait`
    /// elapses uncontested. Starts in [`ClaimState::Claiming`]; the first
    /// [`Self::poll`] emits the initial Address Claimed frame.
    #[must_use]
    pub const fn new(configured_sa: u8, name: u64, claim_wait: Duration) -> Self {
        Self {
            name,
            current_sa: configured_sa,
            state: ClaimState::Claiming,
            claim_wait,
            started_at: None,
            priority: ADDRESS_CLAIM_PRIORITY,
            out: Vec::new(),
        }
    }

    /// This engine's 64-bit NAME.
    #[must_use]
    pub const fn name(&self) -> u64 {
        self.name
    }

    /// Current claim state.
    #[must_use]
    pub const fn state(&self) -> ClaimState {
        self.state
    }

    /// The source address currently held or sought ([`NULL_ADDRESS`] when
    /// in [`ClaimState::CannotClaim`]).
    #[must_use]
    pub const fn source_addr(&self) -> u8 {
        self.current_sa
    }

    /// Drain the outbound frames queued since the last call.
    #[must_use]
    pub fn take_outbound(&mut self) -> Vec<AcOutFrame> {
        std::mem::take(&mut self.out)
    }

    /// Tick the claim-wait timer with the current `now`.
    ///
    /// * On the very first call while `Claiming`, emits the initial
    ///   Address Claimed frame and starts the claim-wait timer.
    /// * Once `claim_wait` has elapsed uncontested, transitions to
    ///   [`ClaimState::Claimed`].
    pub fn poll(&mut self, now: Instant) -> Vec<AcEvent> {
        if self.state != ClaimState::Claiming {
            return Vec::new();
        }
        match self.started_at {
            None => {
                self.started_at = Some(now);
                self.emit_address_claimed(self.current_sa);
                vec![AcEvent::Claiming {
                    source_addr: self.current_sa,
                }]
            }
            Some(start) => {
                if now.duration_since(start) >= self.claim_wait {
                    self.state = ClaimState::Claimed;
                    vec![AcEvent::Claimed {
                        source_addr: self.current_sa,
                    }]
                } else {
                    Vec::new()
                }
            }
        }
    }

    /// Feed one decoded inbound frame. Recognises Address Claimed (PGN
    /// 60928), Request for Address Claimed (PGN 59904), and Commanded
    /// Address (PGN 65240, reassembled 9-byte payload); ignores everything
    /// else.
    pub fn on_frame(&mut self, decoded: &DecodedId, data: &[u8], now: Instant) -> Vec<AcEvent> {
        match decoded.pgn.value() {
            ADDRESS_CLAIMED_PGN => self.on_address_claimed(decoded.source_addr, data),
            REQUEST_PGN => {
                self.on_request(data);
                Vec::new()
            }
            COMMANDED_ADDRESS_PGN => self.on_commanded_address(data, now),
            _ => Vec::new(),
        }
    }

    /// A competing Address Claimed arrived from `competitor_sa` carrying
    /// `data` = the competitor's 8-byte NAME (LE). Arbitrate by NAME
    /// priority (lower NAME wins).
    fn on_address_claimed(&mut self, competitor_sa: u8, data: &[u8]) -> Vec<AcEvent> {
        // Already ceded, or the claim is for a different SA than ours, or a
        // malformed (short) NAME: nothing to arbitrate.
        if self.state == ClaimState::CannotClaim
            || competitor_sa != self.current_sa
            || data.len() < 8
        {
            return Vec::new();
        }
        let competitor_name = u64::from_le_bytes(data[..8].try_into().expect("8 bytes checked"));
        if competitor_name == self.name {
            // Our own echo / an identical NAME — not a real contender.
            return Vec::new();
        }
        if competitor_name < self.name {
            // Competitor has higher priority (lower NAME): we lose. Cede to
            // the null address and emit the Cannot-Claim message.
            self.state = ClaimState::CannotClaim;
            self.current_sa = NULL_ADDRESS;
            self.started_at = None;
            self.emit_cannot_claim();
            vec![AcEvent::CannotClaim]
        } else {
            // We have higher priority (lower NAME): we win. Re-assert our
            // Address Claimed and keep our current state.
            self.emit_address_claimed(self.current_sa);
            Vec::new()
        }
    }

    /// A Request frame arrived. If it requests PGN 60928, (re)send our
    /// current Address Claimed (or the Cannot-Claim null message).
    fn on_request(&mut self, data: &[u8]) {
        if data.len() < 3 || data[..3] != REQUEST_FOR_ADDRESS_CLAIMED {
            return;
        }
        match self.state {
            ClaimState::CannotClaim => self.emit_cannot_claim(),
            ClaimState::Claiming | ClaimState::Claimed => {
                self.emit_address_claimed(self.current_sa);
            }
        }
    }

    /// A reassembled 9-byte Commanded Address arrived: 8-byte target NAME
    /// (LE) + 1 byte new source address. If the NAME is ours, adopt the
    /// new SA and re-enter `Claiming`.
    fn on_commanded_address(&mut self, data: &[u8], now: Instant) -> Vec<AcEvent> {
        if data.len() < 9 {
            return Vec::new();
        }
        let target_name = u64::from_le_bytes(data[..8].try_into().expect("8 bytes checked"));
        if target_name != self.name {
            return Vec::new();
        }
        let new_sa = data[8];
        self.current_sa = new_sa;
        self.state = ClaimState::Claiming;
        // Restart the claim-wait window now and emit the new Address
        // Claimed eagerly so the adoption is observable this step.
        self.started_at = Some(now);
        self.emit_address_claimed(new_sa);
        vec![AcEvent::Claiming {
            source_addr: new_sa,
        }]
    }

    fn emit_address_claimed(&mut self, sa: u8) {
        self.out.push(self.address_claimed_frame(sa));
    }

    fn emit_cannot_claim(&mut self) {
        self.out.push(self.address_claimed_frame(NULL_ADDRESS));
    }

    fn address_claimed_frame(&self, sa: u8) -> AcOutFrame {
        AcOutFrame {
            wire_pgn: Pgn::new(ADDRESS_CLAIMED_PGN).unwrap_or(Pgn::ZERO),
            priority: self.priority,
            source_addr: sa,
            dest_addr: Some(GLOBAL_ADDRESS),
            data: self.name.to_le_bytes(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::{decode_extended_id, encode_extended_id};

    fn decoded(pgn_value: u32, sa: u8) -> DecodedId {
        decode_extended_id(encode_extended_id(
            Pgn::new(pgn_value).unwrap(),
            ADDRESS_CLAIM_PRIORITY,
            sa,
            Some(GLOBAL_ADDRESS),
        ))
    }

    const OUR_SA: u8 = 0x80;
    const OUR_NAME: u64 = 0x0000_0000_0000_1000;

    #[test]
    fn uncontested_claim_emits_then_claims_after_wait() {
        let t0 = Instant::now();
        let mut eng = AddrClaimEngine::new(OUR_SA, OUR_NAME, Duration::from_millis(250));
        assert_eq!(eng.state(), ClaimState::Claiming);

        // First poll: emits the initial Address Claimed and starts timer.
        let ev = eng.poll(t0);
        assert_eq!(
            ev,
            vec![AcEvent::Claiming {
                source_addr: OUR_SA
            }]
        );
        let out = eng.take_outbound();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].wire_pgn.value(), ADDRESS_CLAIMED_PGN);
        assert_eq!(out[0].source_addr, OUR_SA);
        assert_eq!(out[0].dest_addr, Some(GLOBAL_ADDRESS));
        assert_eq!(u64::from_le_bytes(out[0].data), OUR_NAME);

        // Before the wait elapses: still Claiming.
        assert!(eng.poll(t0 + Duration::from_millis(249)).is_empty());
        assert_eq!(eng.state(), ClaimState::Claiming);

        // After the wait: Claimed.
        let ev = eng.poll(t0 + Duration::from_millis(250));
        assert_eq!(
            ev,
            vec![AcEvent::Claimed {
                source_addr: OUR_SA
            }]
        );
        assert_eq!(eng.state(), ClaimState::Claimed);
    }

    #[test]
    fn lower_competitor_name_wins_and_we_cede_to_null() {
        let t0 = Instant::now();
        let mut eng = AddrClaimEngine::new(OUR_SA, OUR_NAME, DEFAULT_CLAIM_WAIT);
        let _ = eng.poll(t0);
        let _ = eng.take_outbound();

        // Competing Address Claimed for the SAME SA with a LOWER (higher
        // priority) NAME → we lose.
        let competitor_name: u64 = OUR_NAME - 1;
        let ev = eng.on_frame(
            &decoded(ADDRESS_CLAIMED_PGN, OUR_SA),
            &competitor_name.to_le_bytes(),
            t0,
        );
        assert_eq!(ev, vec![AcEvent::CannotClaim]);
        assert_eq!(eng.state(), ClaimState::CannotClaim);
        assert_eq!(eng.source_addr(), NULL_ADDRESS);

        // A Cannot-Claim frame (Address Claimed from null 254, our NAME).
        let out = eng.take_outbound();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].source_addr, NULL_ADDRESS);
        assert_eq!(u64::from_le_bytes(out[0].data), OUR_NAME);
    }

    #[test]
    fn higher_competitor_name_loses_and_we_reassert() {
        let t0 = Instant::now();
        let mut eng = AddrClaimEngine::new(OUR_SA, OUR_NAME, DEFAULT_CLAIM_WAIT);
        let _ = eng.poll(t0);
        let _ = eng.take_outbound();

        let competitor_name: u64 = OUR_NAME + 1; // lower priority
        let ev = eng.on_frame(
            &decoded(ADDRESS_CLAIMED_PGN, OUR_SA),
            &competitor_name.to_le_bytes(),
            t0,
        );
        assert!(ev.is_empty());
        assert_eq!(eng.state(), ClaimState::Claiming);
        // We re-asserted our claim.
        let out = eng.take_outbound();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].source_addr, OUR_SA);
    }

    #[test]
    fn responds_to_request_for_address_claimed() {
        let t0 = Instant::now();
        let mut eng = AddrClaimEngine::new(OUR_SA, OUR_NAME, DEFAULT_CLAIM_WAIT);
        let _ = eng.poll(t0);
        let _ = eng.take_outbound();

        let ev = eng.on_frame(
            &decoded(REQUEST_PGN, 0x21),
            &REQUEST_FOR_ADDRESS_CLAIMED,
            t0,
        );
        assert!(ev.is_empty());
        let out = eng.take_outbound();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].wire_pgn.value(), ADDRESS_CLAIMED_PGN);
        assert_eq!(out[0].source_addr, OUR_SA);
    }

    #[test]
    fn cannot_claim_responds_with_null_message() {
        let t0 = Instant::now();
        let mut eng = AddrClaimEngine::new(OUR_SA, OUR_NAME, DEFAULT_CLAIM_WAIT);
        let _ = eng.poll(t0);
        let _ = eng.take_outbound();
        let _ = eng.on_frame(
            &decoded(ADDRESS_CLAIMED_PGN, OUR_SA),
            &(OUR_NAME - 1).to_le_bytes(),
            t0,
        );
        let _ = eng.take_outbound();

        // Request while in CannotClaim → respond from the null address.
        let _ = eng.on_frame(
            &decoded(REQUEST_PGN, 0x21),
            &REQUEST_FOR_ADDRESS_CLAIMED,
            t0,
        );
        let out = eng.take_outbound();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].source_addr, NULL_ADDRESS);
    }

    #[test]
    fn commanded_address_for_our_name_is_adopted() {
        let t0 = Instant::now();
        let mut eng = AddrClaimEngine::new(OUR_SA, OUR_NAME, DEFAULT_CLAIM_WAIT);
        // Drive to Claimed first.
        let _ = eng.poll(t0);
        let _ = eng.take_outbound();
        let _ = eng.poll(t0 + DEFAULT_CLAIM_WAIT);
        assert_eq!(eng.state(), ClaimState::Claimed);

        let new_sa = 0x42u8;
        let mut payload = OUR_NAME.to_le_bytes().to_vec();
        payload.push(new_sa);
        let ev = eng.on_frame(&decoded(COMMANDED_ADDRESS_PGN, 0x00), &payload, t0);
        assert_eq!(
            ev,
            vec![AcEvent::Claiming {
                source_addr: new_sa
            }]
        );
        assert_eq!(eng.state(), ClaimState::Claiming);
        assert_eq!(eng.source_addr(), new_sa);

        // Re-asserted Address Claimed from the new SA.
        let out = eng.take_outbound();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].source_addr, new_sa);
    }

    #[test]
    fn commanded_address_for_other_name_is_ignored() {
        let t0 = Instant::now();
        let mut eng = AddrClaimEngine::new(OUR_SA, OUR_NAME, DEFAULT_CLAIM_WAIT);
        let _ = eng.poll(t0);
        let _ = eng.take_outbound();

        let mut payload = (OUR_NAME ^ 0xFFFF).to_le_bytes().to_vec();
        payload.push(0x42);
        let ev = eng.on_frame(&decoded(COMMANDED_ADDRESS_PGN, 0x00), &payload, t0);
        assert!(ev.is_empty());
        assert_eq!(eng.source_addr(), OUR_SA);
        assert!(eng.take_outbound().is_empty());
    }

    #[test]
    fn gate_tracks_state() {
        let gate = ClaimGate::new();
        assert_eq!(gate.state(), ClaimState::Claiming);
        assert!(!gate.is_claimed());
        gate.set(ClaimState::Claimed);
        assert!(gate.is_claimed());
        gate.set(ClaimState::CannotClaim);
        assert!(!gate.is_claimed());
        assert_eq!(gate.state(), ClaimState::CannotClaim);
    }
}
