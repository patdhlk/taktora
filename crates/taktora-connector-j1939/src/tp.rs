//! Reusable userspace Transport-Protocol (TP) session engine. `BB_0101`,
//! `REQ_0892`, `REQ_0895`, `REQ_0896`, `ADR_0109`.
//!
//! This module is the shared backbone for every multi-packet J1939
//! transport: issue #123 (this one) lands **BAM** (Broadcast Announce
//! Message), #124 extends it with **RTS/CTS** connection-mode flow
//! control, and #125 adds **ETP** (Extended Transport Protocol) for
//! payloads above 1785 bytes. They all reuse:
//!
//! * the bounded inbound-session table ([`TpEngine`] keyed by
//!   `(source_addr, transported_pgn)`),
//! * the [`TpTimers`] set with J1939-21 defaults,
//! * the clock-stepped [`TpEngine::on_frame`] / [`TpEngine::poll_timeouts`]
//!   API (an explicit `now: Instant` makes timeouts testable with no real
//!   sleeps),
//! * and the [`TpEvent`] / [`TpAbortReason`] vocabulary the dispatcher
//!   turns into iceoryx2 publishes and `HealthEvent`s.
//!
//! ## Engine design (pure-ish state machine)
//!
//! [`TpEngine`] holds no I/O, no clock, and no iceoryx2 handles. It is
//! stepped by the dispatcher:
//!
//! * inbound TP.CM / TP.DT frames feed [`TpEngine::on_frame`] →
//!   `Vec<TpEvent>`,
//! * every dispatcher iteration calls [`TpEngine::poll_timeouts`] with the
//!   current `now` → `Vec<TpEvent>`,
//! * outbound whole messages are split by [`TpEngine::segment_outbound`]
//!   into ready-to-encode [`TpOutFrame`]s.
//!
//! The dispatcher (see [`crate::dispatcher`]) owns the side effects:
//! [`TpEvent::Completed`] publishes the reassembled payload to the
//! matching channel; [`TpEvent::Aborted`] / [`TpEvent::SessionRefused`]
//! become `HealthEvent`s (`REQ_0895`/`REQ_0896`) — never a silent drop.
//!
//! ## BAM vs connection-mode (#124) abort semantics
//!
//! BAM is **connectionless**: there is no TP.Conn_Abort frame on the wire
//! for a broadcast. A BAM timeout (T1) or a session-bound refusal is
//! therefore surfaced **locally** as a [`TpEvent`] → `HealthEvent`, and no
//! Abort frame is transmitted. Issue #124's RTS/CTS path will, by
//! contrast, emit a real TP.Conn_Abort frame (control byte `0xFF`) onto
//! the bus carrying the same [`TpAbortReason`]. The [`TpAbortReason`] enum
//! and the event vocabulary are deliberately shared so #124 only adds the
//! wire-frame emission, not new bookkeeping.
//!
//! ## Extension seams
//!
//! * **#124 (RTS/CTS):** `on_frame` currently only recognises the BAM
//!   control byte ([`BAM_CONTROL`]) inside a TP.CM. The `else` branch is
//!   the seam where RTS (`0x10`), CTS (`0x11`), `EndOfMsgAck` (`0x13`),
//!   and Conn_Abort (`0xFF`) control bytes plug in, plus an outbound
//!   connection-state table parallel to the inbound one. The
//!   T2/T3/T4/Tr/Th timers are already defined in [`TpTimers`] for that
//!   flow.
//! * **#125 (ETP):** ETP uses PGNs 0xC800 (ETP.CM) / 0xC700 (ETP.DT) and
//!   a 32-bit byte offset instead of a 7-byte packet index; it lands on
//!   the slice channel. A parallel `on_etp_frame` (or an extended
//!   `SessionKind`-tagged session) reuses the same bounded table, timer
//!   set, and health plumbing.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::decode::{DecodedId, GLOBAL_ADDRESS};
use crate::routing::Pgn;

/// TP.CM (Connection Management) PGN — `0xEC00` = 60416. PDU1; BAM uses
/// the global destination `0xFF`.
pub const TP_CM_PGN: u32 = 60416;

/// TP.DT (Data Transfer) PGN — `0xEB00` = 60160. PDU1.
pub const TP_DT_PGN: u32 = 60160;

/// TP.CM control byte identifying a BAM (Broadcast Announce Message).
pub const BAM_CONTROL: u8 = 0x20;

/// TP.CM control byte for **RTS** (Request To Send) — opens a
/// connection-mode (RTS/CTS) transfer (#124). Sender → receiver.
pub const RTS_CONTROL: u8 = 0x10;

/// TP.CM control byte for **CTS** (Clear To Send) — grants a flow-control
/// window (#124). Receiver → sender. A `num_packets` of `0` means
/// "wait/hold".
pub const CTS_CONTROL: u8 = 0x11;

/// TP.CM control byte for **EndOfMsgAck** (End Of Message Acknowledge) —
/// every packet of a connection-mode transfer was received (#124).
/// Receiver → sender.
pub const EOMA_CONTROL: u8 = 0x13;

/// TP.CM control byte for **Conn_Abort** (Connection Abort) — either side
/// aborts a connection-mode transfer (#124), carrying a [`TpAbortReason`].
pub const CONN_ABORT_CONTROL: u8 = 0xFF;

/// `max_packets_per_cts` value meaning "no limit" in an RTS — the receiver
/// chooses its window freely.
pub const CTS_NO_LIMIT: u8 = 0xFF;

/// Default receiver CTS window: the number of TP.DT packets granted per
/// CTS when reassembling a connection-mode transfer (#124). Overridable
/// with [`TpEngine::with_cts_window`]. Reused for ETP (#125) burst grants.
pub const DEFAULT_CTS_WINDOW: u8 = 16;

// ---- ETP (Extended Transport Protocol), #125, REQ_0894 / REQ_0903 ----

/// ETP.CM (Connection Management) PGN — `0xC800` = 51200. PDU1
/// (destination-specific). Carries ETP.RTS / CTS / DPO / EndOfMsgAck /
/// Abort control frames (#125).
pub const ETP_CM_PGN: u32 = 51200;

/// ETP.DT (Data Transfer) PGN — `0xC700` = 50944. PDU1. Carries the
/// `[seq_no, d0..d6]` data packets; the absolute packet number is derived
/// from the current ETP.DPO offset (#125).
pub const ETP_DT_PGN: u32 = 50944;

/// ETP.CM control byte for **ETP.RTS** (Request To Send) — sender →
/// receiver. Bytes 1..4 are the 32-bit little-endian total size
/// (`1786..=117_440_505`), bytes 5..7 the transported PGN (#125).
pub const ETP_RTS_CONTROL: u8 = 0x14;

/// ETP.CM control byte for **ETP.CTS** (Clear To Send) — receiver →
/// sender. byte1 = number of packets to send in this burst (0 = wait);
/// bytes 2..4 = next packet number, 24-bit LE, 1-based (#125).
pub const ETP_CTS_CONTROL: u8 = 0x15;

/// ETP.CM control byte for **ETP.DPO** (Data Packet Offset) — sender →
/// receiver, sent immediately before each ETP.DT burst. byte1 = number of
/// ETP.DT packets that follow; bytes 2..4 = packet offset, 24-bit LE. The
/// absolute packet number of a following ETP.DT is `offset + seq_no`
/// (`seq_no` resets to 1 each burst) (#125).
pub const ETP_DPO_CONTROL: u8 = 0x16;

/// ETP.CM control byte for **ETP.EndOfMsgAck** — receiver → sender; every
/// packet arrived (#125).
pub const ETP_EOMA_CONTROL: u8 = 0x17;

/// ETP.CM control byte for **ETP.Abort** — either side. `[0xFF, reason,
/// 0xFF, 0xFF, 0xFF, pgn..]`, carrying a [`TpAbortReason`] on the ETP.CM
/// PGN (#125). Numerically identical to the RTS/CTS [`CONN_ABORT_CONTROL`]
/// but distinguished by riding [`ETP_CM_PGN`].
pub const ETP_ABORT_CONTROL: u8 = 0xFF;

/// Smallest ETP payload, in bytes — one more than the BAM/RTS-CTS
/// [`TP_MAX_PAYLOAD`]. Anything `<= 1785` belongs on the connection-mode
/// (RTS/CTS) or BAM path, not ETP.
pub const ETP_MIN_PAYLOAD: usize = TP_MAX_PAYLOAD + 1;

/// J1939-21 ETP protocol maximum payload, in bytes (a 32-bit packet count
/// of `0x00FF_FFFF` packets × 7 bytes). Reassembly must NEVER grow
/// unbounded toward this ceiling — it is bounded by `max_etp_bytes`
/// instead (`REQ_0903`).
pub const ETP_PROTOCOL_MAX: usize = 117_440_505;

/// Default `max_etp_bytes` ceiling — 16 MiB. Generous enough for real ETP
/// transfers yet far below the ~117 MB protocol maximum, keeping the
/// bounded-allocation invariant auditable (`REQ_0903`, `ADR_0109`).
/// Overridable via [`TpEngine::with_max_etp_bytes`] /
/// [`crate::options::J1939ConnectorOptions`].
pub const DEFAULT_MAX_ETP_BYTES: usize = 16 * 1024 * 1024;

/// Minimum BAM/TP payload size in bytes (8 bytes or fewer is a single
/// classical frame).
pub const TP_MIN_PAYLOAD: usize = 9;

/// Maximum BAM/RTS-CTS payload size in bytes (`REQ_0892`). ETP (#125)
/// carries larger messages.
pub const TP_MAX_PAYLOAD: usize = 1785;

/// Bytes of payload carried per TP.DT data-transfer frame.
pub const TP_DT_DATA_LEN: usize = 7;

/// Default maximum concurrent inbound TP sessions per interface
/// (`REQ_0896`). Overridable via
/// [`crate::options::J1939ConnectorOptions`].
pub const DEFAULT_MAX_TP_SESSIONS: usize = 8;

/// J1939-21 transport-protocol timer set with standard defaults
/// (`REQ_0895`). Every duration is overridable via the options builder.
///
/// BAM (this issue) only exercises [`TpTimers::t1`] on the receive side;
/// the connection-mode timers (`t2`/`t3`/`t4`/`tr`/`th`) are defined here
/// with their standard defaults so the RTS/CTS path (#124) can consume
/// them without an options-surface change.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TpTimers {
    /// Tr — response time, 200 ms. Connection-mode pacing (#124).
    pub tr: Duration,
    /// Th — holding time, 500 ms. Connection-mode pacing (#124).
    pub th: Duration,
    /// T1 — 750 ms. Max gap between consecutive received TP.DT packets on
    /// the receive side. Exceeded ⇒ abort + `HealthEvent`. The BAM
    /// receive-side timer this issue enforces.
    pub t1: Duration,
    /// T2 — 1250 ms. RTS/CTS flow control (#124).
    pub t2: Duration,
    /// T3 — 1250 ms. RTS/CTS flow control (#124).
    pub t3: Duration,
    /// T4 — 1050 ms. RTS/CTS flow control (#124).
    pub t4: Duration,
}

impl Default for TpTimers {
    fn default() -> Self {
        Self {
            tr: Duration::from_millis(200),
            th: Duration::from_millis(500),
            t1: Duration::from_millis(750),
            t2: Duration::from_millis(1250),
            t3: Duration::from_millis(1250),
            t4: Duration::from_millis(1050),
        }
    }
}

/// J1939-21 TP.Conn_Abort reason code (the `u8` carried in a connection
/// abort). Shared between the local-only BAM surfacing and the on-wire
/// abort #124 will emit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum TpAbortReason {
    /// 1 — already in one or more connection-managed sessions.
    AlreadyInSession = 1,
    /// 2 — system resources were needed for another task. Used for the
    /// session-bound refusal (`REQ_0896`).
    Resources = 2,
    /// 3 — a timeout occurred. Used for timer aborts (`REQ_0895`).
    Timeout = 3,
    /// 4 — any other reason.
    Other = 4,
}

impl TpAbortReason {
    /// The J1939-21 numeric reason code.
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }
}

impl core::fmt::Display for TpAbortReason {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let text = match self {
            Self::AlreadyInSession => "already in a connection-managed session",
            Self::Resources => "system resources needed for another task",
            Self::Timeout => "a timeout occurred",
            Self::Other => "other",
        };
        write!(f, "{text} (code {})", self.as_u8())
    }
}

/// An event produced by the [`TpEngine`] for the dispatcher to act on.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TpEvent {
    /// A complete multi-packet payload was reassembled and should be
    /// published to the channel matching `(pgn, source_addr)`.
    Completed {
        /// Transported PGN the payload is delivered as.
        pgn: Pgn,
        /// Source address of the transmitting node.
        source_addr: u8,
        /// Reassembled payload (`9..=1785` bytes for BAM).
        payload: Vec<u8>,
    },
    /// An in-progress session was aborted (e.g. a T1 timeout). The
    /// dispatcher surfaces this as a `HealthEvent` (`REQ_0895`).
    Aborted {
        /// Transported PGN of the aborted session.
        pgn: Pgn,
        /// Source address of the aborted session.
        source_addr: u8,
        /// Why the session aborted.
        reason: TpAbortReason,
    },
    /// A new inbound session was refused because the per-interface
    /// concurrent-session cap was reached (`REQ_0896`). No session is
    /// allocated; the dispatcher surfaces a `HealthEvent`.
    SessionRefused {
        /// Transported PGN of the refused session.
        pgn: Pgn,
        /// Source address whose session was refused.
        source_addr: u8,
        /// Always [`TpAbortReason::Resources`] for the session-bound cap.
        reason: TpAbortReason,
    },
}

/// One ready-to-encode outbound TP frame produced by
/// [`TpEngine::segment_outbound`]. The dispatcher encodes the 29-bit id
/// from `(wire_pgn, priority, source_addr, dest_addr)` and sends `data`
/// as a single classical CAN frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TpOutFrame {
    /// Wire PGN — [`TP_CM_PGN`] for the announce, [`TP_DT_PGN`] for data.
    pub wire_pgn: Pgn,
    /// TX priority for the encoded identifier.
    pub priority: u8,
    /// Source address of the transmitting node.
    pub source_addr: u8,
    /// Destination address — `Some(0xFF)` (global) for BAM.
    pub dest_addr: Option<u8>,
    /// The 8 data bytes of the frame (trailing bytes padded `0xFF`).
    pub data: [u8; 8],
}

/// Failure modes of [`TpEngine::segment_outbound`].
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum TpError {
    /// Payload is outside the BAM range (`9..=1785` bytes). Smaller
    /// payloads belong on a single-frame channel; larger ones are ETP
    /// (#125).
    #[error("TP payload length {0} out of BAM range {TP_MIN_PAYLOAD}..={TP_MAX_PAYLOAD}")]
    PayloadOutOfRange(usize),
    /// ETP payload is outside `1786..=max_etp_bytes` (#125). Below 1786 it
    /// belongs on the BAM/RTS-CTS path; above the cap it is refused so
    /// reassembly never grows unbounded (`REQ_0894`/`REQ_0903`).
    #[error("ETP payload length {len} out of range {ETP_MIN_PAYLOAD}..={max} (max_etp_bytes)")]
    EtpPayloadOutOfRange {
        /// The offending payload length.
        len: usize,
        /// The configured `max_etp_bytes` ceiling.
        max: usize,
    },
}

/// In-progress inbound reassembly state for one BAM session.
#[derive(Debug)]
struct InboundSession {
    transported_pgn: Pgn,
    source_addr: u8,
    total_size: usize,
    num_packets: u8,
    /// Next TP.DT sequence number expected (1-based).
    next_seq: u8,
    buf: Vec<u8>,
    /// Time the last frame (TP.CM open or a TP.DT) was accepted — the T1
    /// gap is measured from here.
    last_activity: Instant,
}

/// In-progress INBOUND connection-mode (RTS/CTS) reassembly state — we
/// are the **receiver**. Keyed by `(peer_sa, transported_pgn)`. `REQ_0893`.
#[derive(Debug)]
struct InboundConnSession {
    transported_pgn: Pgn,
    /// The transmitting node (RTS source); also the CTS/EndOfMsgAck dest.
    peer_sa: u8,
    /// Our own address (RTS destination); the CTS/EndOfMsgAck source.
    our_sa: u8,
    /// Priority echoed onto our CTS / EndOfMsgAck / Conn_Abort replies.
    priority: u8,
    total_size: usize,
    total_packets: u8,
    /// Max packets the sender said it can send per CTS (`0xFF` = no limit).
    sender_max_per_cts: u8,
    /// Next TP.DT sequence number expected (1-based).
    next_seq: u8,
    /// Packets still outstanding in the currently-granted CTS window.
    window_remaining: u8,
    buf: Vec<u8>,
    /// Time of the last accepted frame; the T2 gap is measured from here.
    last_activity: Instant,
}

/// In-progress OUTBOUND connection-mode (RTS/CTS) segmentation state — we
/// are the **sender**. Keyed by `(dest_sa, transported_pgn)`. `REQ_0893`.
#[derive(Debug)]
struct OutboundConnSession {
    transported_pgn: Pgn,
    /// Our own address (RTS/TP.DT source).
    our_sa: u8,
    /// The receiving node (RTS/TP.DT destination); the CTS source.
    dest_sa: u8,
    priority: u8,
    /// The whole message being segmented; TP.DT bursts slice into this.
    payload: Vec<u8>,
    total_packets: u8,
    /// Time of the last activity (RTS sent / burst sent); T3 measured from
    /// here while waiting for the next CTS.
    last_activity: Instant,
}

/// In-progress INBOUND ETP reassembly state — we are the **receiver**.
/// Keyed by `(peer_sa, transported_pgn)` (#125). The reassembly buffer is
/// sized to the announced `total_size`, which the engine has already
/// verified is `<= max_etp_bytes` (`REQ_0894`/`REQ_0903`).
#[derive(Debug)]
struct InboundEtpSession {
    transported_pgn: Pgn,
    /// The transmitting node (ETP.RTS source); the CTS/EndOfMsgAck dest.
    peer_sa: u8,
    /// Our own address (ETP.RTS destination); the CTS/EndOfMsgAck source.
    our_sa: u8,
    priority: u8,
    total_size: usize,
    total_packets: u32,
    /// Next ABSOLUTE packet number expected (1-based, may exceed 255).
    next_packet: u32,
    /// Packet offset declared by the most recent ETP.DPO.
    dpo_offset: u32,
    /// ETP.DT packets still expected in the current DPO burst.
    burst_remaining: u32,
    /// Reassembly buffer, pre-sized to `total_size`.
    buf: Vec<u8>,
    /// Time of the last accepted frame; T2 is measured from here.
    last_activity: Instant,
}

/// In-progress OUTBOUND ETP segmentation state — we are the **sender**.
/// Keyed by `(dest_sa, transported_pgn)` (#125).
#[derive(Debug)]
struct OutboundEtpSession {
    transported_pgn: Pgn,
    our_sa: u8,
    dest_sa: u8,
    priority: u8,
    /// The whole message; ETP.DT bursts slice into this by byte offset.
    payload: Vec<u8>,
    total_packets: u32,
    /// Time of the last activity; T3 is measured from here.
    last_activity: Instant,
}

/// Reusable, bounded, clock-stepped TP session engine (`BB_0101`).
///
/// Holds the inbound reassembly table, the [`TpTimers`], and the
/// per-interface concurrent-session cap. One engine instance is owned per
/// interface dispatcher (BAM is one active session per source address, so
/// the `(source_addr, pgn)` key uniquely identifies a session).
#[derive(Debug)]
pub struct TpEngine {
    sessions: HashMap<(u8, Pgn), InboundSession>,
    /// INBOUND connection-mode reassembly, keyed by `(peer_sa, pgn)` (#124).
    rx_conn: HashMap<(u8, Pgn), InboundConnSession>,
    /// OUTBOUND connection-mode segmentation, keyed by `(dest_sa, pgn)`
    /// (#124).
    tx_conn: HashMap<(u8, Pgn), OutboundConnSession>,
    /// Frames the engine produced in response to inbound frames / timers
    /// (CTS, EndOfMsgAck, TP.DT bursts, Conn_Abort) — drained with
    /// [`TpEngine::take_outbound`] (#124).
    pending_out: Vec<TpOutFrame>,
    /// INBOUND ETP reassembly, keyed by `(peer_sa, pgn)` (#125).
    etp_rx: HashMap<(u8, Pgn), InboundEtpSession>,
    /// OUTBOUND ETP segmentation, keyed by `(dest_sa, pgn)` (#125).
    etp_tx: HashMap<(u8, Pgn), OutboundEtpSession>,
    /// Packets granted per CTS when we are the receiver (#124, reused for
    /// ETP burst grants #125).
    cts_window: u8,
    timers: TpTimers,
    max_sessions: usize,
    outbound_pacing: Duration,
    /// Hard ceiling on an ETP reassembly buffer (`REQ_0894`/`REQ_0903`).
    /// An inbound ETP.RTS announcing a larger total is aborted with
    /// [`TpAbortReason::Resources`] before any buffer is allocated.
    max_etp_bytes: usize,
}

impl TpEngine {
    /// Construct an engine with the given timer set and per-interface
    /// concurrent-session cap. Outbound inter-packet pacing defaults to
    /// zero (frames emitted back-to-back, correct for the mock/test bus);
    /// set a `>= 50 ms` pacing with [`Self::with_outbound_pacing`] for a
    /// real bus.
    #[must_use]
    pub fn new(timers: TpTimers, max_sessions: usize) -> Self {
        Self {
            sessions: HashMap::new(),
            rx_conn: HashMap::new(),
            tx_conn: HashMap::new(),
            etp_rx: HashMap::new(),
            etp_tx: HashMap::new(),
            pending_out: Vec::new(),
            cts_window: DEFAULT_CTS_WINDOW,
            timers,
            max_sessions: max_sessions.max(1),
            outbound_pacing: Duration::ZERO,
            max_etp_bytes: DEFAULT_MAX_ETP_BYTES,
        }
    }

    /// Builder-style override of the ETP reassembly ceiling
    /// (`REQ_0894`/`REQ_0903`). An inbound ETP.RTS announcing a total size
    /// above this is aborted with [`TpAbortReason::Resources`] before any
    /// buffer is allocated, and an outbound [`Self::start_outbound_etp`]
    /// payload above it is rejected. Clamped to a minimum of
    /// [`ETP_MIN_PAYLOAD`] so a sub-1786-byte cap can never make ETP
    /// impossible.
    #[must_use]
    pub const fn with_max_etp_bytes(mut self, max: usize) -> Self {
        self.max_etp_bytes = if max < ETP_MIN_PAYLOAD {
            ETP_MIN_PAYLOAD
        } else {
            max
        };
        self
    }

    /// The configured ETP reassembly ceiling (`REQ_0894`).
    #[must_use]
    pub const fn max_etp_bytes(&self) -> usize {
        self.max_etp_bytes
    }

    /// Builder-style override of the receiver CTS window — the number of
    /// TP.DT packets granted per CTS when reassembling a connection-mode
    /// transfer (#124). A larger total than this forces MULTIPLE CTS rounds
    /// (`REQ_0893`). Clamped to a minimum of `1`.
    #[must_use]
    pub const fn with_cts_window(mut self, window: u8) -> Self {
        self.cts_window = if window == 0 { 1 } else { window };
        self
    }

    /// Builder-style override of the outbound BAM inter-packet pacing
    /// knob. Real J1939 requires a `>= 50 ms` gap between BAM TP.DT
    /// frames; the dispatcher consults [`Self::outbound_pacing`] when it
    /// drains segmented frames. The mock/test path emits back-to-back.
    #[must_use]
    pub const fn with_outbound_pacing(mut self, pacing: Duration) -> Self {
        self.outbound_pacing = pacing;
        self
    }

    /// The configured timer set.
    #[must_use]
    pub const fn timers(&self) -> &TpTimers {
        &self.timers
    }

    /// The configured per-interface concurrent inbound-session cap.
    #[must_use]
    pub const fn max_sessions(&self) -> usize {
        self.max_sessions
    }

    /// The configured outbound BAM inter-packet pacing.
    #[must_use]
    pub const fn outbound_pacing(&self) -> Duration {
        self.outbound_pacing
    }

    /// Number of currently-allocated inbound sessions. Tests assert this
    /// stays bounded by [`Self::max_sessions`] (`REQ_0896`: no unbounded
    /// allocation).
    #[must_use]
    pub fn active_inbound_sessions(&self) -> usize {
        self.sessions.len()
    }

    /// Number of currently-open INBOUND connection-mode (RTS/CTS) sessions
    /// (#124). Tests assert this is freed on EndOfMsgAck / Conn_Abort /
    /// timeout.
    #[must_use]
    pub fn active_inbound_connections(&self) -> usize {
        self.rx_conn.len()
    }

    /// Number of currently-open OUTBOUND connection-mode (RTS/CTS) sessions
    /// (#124). Tests assert this is freed on EndOfMsgAck / Conn_Abort /
    /// timeout.
    #[must_use]
    pub fn active_outbound_connections(&self) -> usize {
        self.tx_conn.len()
    }

    /// Number of currently-open INBOUND ETP reassembly sessions (#125).
    /// Tests assert this stays `0` for an oversize-aborted announce (no
    /// reassembly buffer allocated, `REQ_0903`).
    #[must_use]
    pub fn active_inbound_etp_sessions(&self) -> usize {
        self.etp_rx.len()
    }

    /// Number of currently-open OUTBOUND ETP segmentation sessions (#125).
    #[must_use]
    pub fn active_outbound_etp_sessions(&self) -> usize {
        self.etp_tx.len()
    }

    /// Total inbound sessions (BAM reassembly + connection-mode
    /// reassembly + ETP reassembly) counted against the per-interface
    /// concurrency cap (`REQ_0896`).
    fn total_inbound(&self) -> usize {
        self.sessions.len() + self.rx_conn.len() + self.etp_rx.len()
    }

    /// Drain the frames the engine produced since the last drain (CTS,
    /// EndOfMsgAck, TP.DT bursts, Conn_Abort). The dispatcher encodes and
    /// transmits each one (#124). BAM never queues here, so BAM callers see
    /// an empty drain.
    #[must_use]
    pub fn take_outbound(&mut self) -> Vec<TpOutFrame> {
        std::mem::take(&mut self.pending_out)
    }

    /// Feed one decoded inbound TP.CM / TP.DT frame. Non-TP PGNs return an
    /// empty event list (the dispatcher routes them as single frames).
    ///
    /// `now` is the explicit time source — the engine never reads the
    /// clock itself, so timeouts are deterministic in tests.
    #[must_use]
    pub fn on_frame(&mut self, decoded: &DecodedId, data: &[u8], now: Instant) -> Vec<TpEvent> {
        match decoded.pgn.value() {
            TP_CM_PGN => self.on_cm(decoded, data, now),
            TP_DT_PGN => self.on_dt(decoded, data, now),
            ETP_CM_PGN => self.on_etp_cm(decoded, data, now),
            ETP_DT_PGN => self.on_etp_dt(decoded, data, now),
            _ => Vec::new(),
        }
    }

    /// Dispatch a TP.CM frame by its control byte. BAM (`0x20`) reassembles
    /// broadcast; RTS/CTS/EndOfMsgAck/Conn_Abort drive connection mode
    /// (#124, `REQ_0893`).
    fn on_cm(&mut self, decoded: &DecodedId, data: &[u8], now: Instant) -> Vec<TpEvent> {
        if data.len() < 8 {
            return Vec::new();
        }
        match data[0] {
            BAM_CONTROL => self.on_bam_cm(decoded.source_addr, data, now),
            RTS_CONTROL => self.on_rts(decoded, data, now),
            CTS_CONTROL => self.on_cts(decoded, data, now),
            EOMA_CONTROL => self.on_end_of_msg_ack(decoded, data),
            CONN_ABORT_CONTROL => self.on_conn_abort(decoded, data),
            _ => Vec::new(),
        }
    }

    /// Handle a TP.CM(BAM) announce — broadcast reassembly (#123).
    fn on_bam_cm(&mut self, source_addr: u8, data: &[u8], now: Instant) -> Vec<TpEvent> {
        let total_size = usize::from(u16::from_le_bytes([data[1], data[2]]));
        let num_packets = data[3];
        let pgn_value = u32::from(data[5]) | (u32::from(data[6]) << 8) | (u32::from(data[7]) << 16);
        let Ok(pgn) = Pgn::new(pgn_value) else {
            return Vec::new();
        };
        // Reject malformed announcements rather than allocating garbage.
        if !(TP_MIN_PAYLOAD..=TP_MAX_PAYLOAD).contains(&total_size) {
            return Vec::new();
        }
        let expected_packets = total_size.div_ceil(TP_DT_DATA_LEN);
        if usize::from(num_packets) != expected_packets {
            return Vec::new();
        }

        let key = (source_addr, pgn);
        // REQ_0896: a *new* session beyond the cap is refused — no
        // allocation. An announce for an existing key restarts that
        // session in place (no net growth). Connection-mode reassembly
        // sessions count against the same cap (`total_inbound`).
        if !self.sessions.contains_key(&key) && self.total_inbound() >= self.max_sessions {
            return vec![TpEvent::SessionRefused {
                pgn,
                source_addr,
                reason: TpAbortReason::Resources,
            }];
        }

        self.sessions.insert(
            key,
            InboundSession {
                transported_pgn: pgn,
                source_addr,
                total_size,
                num_packets,
                next_seq: 1,
                buf: Vec::with_capacity(total_size),
                last_activity: now,
            },
        );
        Vec::new()
    }

    /// Dispatch a TP.DT data frame. BAM TP.DT is broadcast (destination
    /// `0xFF`); connection-mode TP.DT is destination-specific (#124), so
    /// the destination address selects the reassembly path.
    fn on_dt(&mut self, decoded: &DecodedId, data: &[u8], now: Instant) -> Vec<TpEvent> {
        if data.is_empty() {
            return Vec::new();
        }
        match decoded.dest_addr {
            Some(GLOBAL_ADDRESS) | None => self.on_bam_dt(decoded.source_addr, data, now),
            Some(_) => self.on_conn_dt(decoded.source_addr, data, now),
        }
    }

    /// Handle a TP.DT(BAM) frame. BAM is one active session per source
    /// address, so the session is located by source address alone (the
    /// TP.DT frame carries no transported PGN).
    fn on_bam_dt(&mut self, source_addr: u8, data: &[u8], now: Instant) -> Vec<TpEvent> {
        let seq = data[0];
        let Some(key) = self
            .sessions
            .keys()
            .find(|(sa, _)| *sa == source_addr)
            .copied()
        else {
            return Vec::new();
        };
        let session = self
            .sessions
            .get_mut(&key)
            .expect("session key just located");

        // Drop out-of-order / duplicate sequence numbers (e.g. a stale
        // retransmit). Real BAM is strictly sequential.
        if seq != session.next_seq {
            return Vec::new();
        }

        let remaining = session.total_size.saturating_sub(session.buf.len());
        let available = data.len().saturating_sub(1);
        let take = remaining.min(TP_DT_DATA_LEN).min(available);
        session.buf.extend_from_slice(&data[1..=take]);
        session.last_activity = now;
        session.next_seq = session.next_seq.wrapping_add(1);

        if seq >= session.num_packets {
            let done = self
                .sessions
                .remove(&key)
                .expect("session key just located");
            return vec![TpEvent::Completed {
                pgn: done.transported_pgn,
                source_addr: done.source_addr,
                payload: done.buf,
            }];
        }
        Vec::new()
    }

    // ---- Connection mode (RTS/CTS), #124, REQ_0893 ------------------

    /// Begin an OUTBOUND connection-mode transfer: register the session
    /// and return the RTS (Request To Send) frame to transmit. The
    /// transfer then advances as CTS frames arrive via [`Self::on_frame`]
    /// (each granting a window of TP.DT to drain via
    /// [`Self::take_outbound`]), completing on the peer's EndOfMsgAck.
    ///
    /// Distinct from BAM's [`Self::segment_outbound`] (which emits every
    /// frame at once with no flow control).
    ///
    /// # Errors
    ///
    /// [`TpError::PayloadOutOfRange`] when `payload.len()` is outside
    /// `9..=1785`.
    pub fn start_outbound_connection(
        &mut self,
        transported_pgn: Pgn,
        priority: u8,
        our_sa: u8,
        dest_addr: u8,
        payload: &[u8],
        now: Instant,
    ) -> Result<Vec<TpOutFrame>, TpError> {
        let size = payload.len();
        if !(TP_MIN_PAYLOAD..=TP_MAX_PAYLOAD).contains(&size) {
            return Err(TpError::PayloadOutOfRange(size));
        }
        let total_packets = size.div_ceil(TP_DT_DATA_LEN) as u8;
        self.tx_conn.insert(
            (dest_addr, transported_pgn),
            OutboundConnSession {
                transported_pgn,
                our_sa,
                dest_sa: dest_addr,
                priority,
                payload: payload.to_vec(),
                total_packets,
                last_activity: now,
            },
        );
        Ok(vec![rts_frame(
            our_sa,
            dest_addr,
            priority,
            transported_pgn,
            size,
            total_packets,
            CTS_NO_LIMIT,
        )])
    }

    /// Handle an inbound RTS — we are the receiver. Open a reassembly
    /// session and queue the first CTS window.
    fn on_rts(&mut self, decoded: &DecodedId, data: &[u8], now: Instant) -> Vec<TpEvent> {
        let Some(p) = parse_conn_rts(decoded, data) else {
            return Vec::new();
        };

        let key = (p.peer_sa, p.pgn);
        // REQ_0896: refuse a new session past the cap — emit a real
        // Conn_Abort onto the wire (connection mode is acknowledged) plus
        // the local SessionRefused event.
        if !self.rx_conn.contains_key(&key) && self.total_inbound() >= self.max_sessions {
            self.pending_out.push(conn_abort_frame(
                p.our_sa,
                p.peer_sa,
                p.priority,
                p.pgn,
                TpAbortReason::Resources,
            ));
            return vec![TpEvent::SessionRefused {
                pgn: p.pgn,
                source_addr: p.peer_sa,
                reason: TpAbortReason::Resources,
            }];
        }

        let window = grant_window(p.total_packets, self.cts_window, p.sender_max_per_cts);
        self.rx_conn.insert(
            key,
            InboundConnSession {
                transported_pgn: p.pgn,
                peer_sa: p.peer_sa,
                our_sa: p.our_sa,
                priority: p.priority,
                total_size: p.total_size,
                total_packets: p.total_packets,
                sender_max_per_cts: p.sender_max_per_cts,
                next_seq: 1,
                window_remaining: window,
                buf: Vec::with_capacity(p.total_size),
                last_activity: now,
            },
        );
        self.pending_out
            .push(cts_frame(p.our_sa, p.peer_sa, p.priority, p.pgn, window, 1));
        Vec::new()
    }

    /// Handle an inbound connection-mode TP.DT — we are the receiver.
    /// Append the packet; on window exhaustion grant the next CTS; on the
    /// final packet emit EndOfMsgAck and complete.
    fn on_conn_dt(&mut self, peer_sa: u8, data: &[u8], now: Instant) -> Vec<TpEvent> {
        let seq = data[0];
        let Some(key) = self.rx_conn.keys().find(|(sa, _)| *sa == peer_sa).copied() else {
            return Vec::new();
        };

        /// What to do after appending a packet (computed while the session
        /// is borrowed, applied afterwards to keep the borrows disjoint).
        enum Act {
            Drop,
            Wait,
            Complete,
            NextWindow { window: u8, next_seq: u8 },
        }

        let act = {
            let s = self.rx_conn.get_mut(&key).expect("key just located");
            if seq == s.next_seq {
                let remaining = s.total_size.saturating_sub(s.buf.len());
                let available = data.len().saturating_sub(1);
                let take = remaining.min(TP_DT_DATA_LEN).min(available);
                s.buf.extend_from_slice(&data[1..=take]);
                s.last_activity = now;
                s.next_seq = s.next_seq.wrapping_add(1);
                s.window_remaining = s.window_remaining.saturating_sub(1);
                if seq >= s.total_packets {
                    Act::Complete
                } else if s.window_remaining == 0 {
                    let remaining_packets = s.total_packets - (s.next_seq - 1);
                    let window =
                        grant_window(remaining_packets, self.cts_window, s.sender_max_per_cts);
                    s.window_remaining = window;
                    Act::NextWindow {
                        window,
                        next_seq: s.next_seq,
                    }
                } else {
                    Act::Wait
                }
            } else {
                // Out-of-order / duplicate: drop (real RTS/CTS is sequential).
                Act::Drop
            }
        };

        match act {
            Act::Drop | Act::Wait => Vec::new(),
            Act::Complete => {
                let done = self.rx_conn.remove(&key).expect("key just located");
                self.pending_out.push(end_of_msg_ack_frame(
                    done.our_sa,
                    done.peer_sa,
                    done.priority,
                    done.transported_pgn,
                    done.total_size,
                    done.total_packets,
                ));
                vec![TpEvent::Completed {
                    pgn: done.transported_pgn,
                    source_addr: done.peer_sa,
                    payload: done.buf,
                }]
            }
            Act::NextWindow { window, next_seq } => {
                let (our_sa, peer, prio, pgn) = {
                    let s = self.rx_conn.get(&key).expect("key just located");
                    (s.our_sa, s.peer_sa, s.priority, s.transported_pgn)
                };
                self.pending_out
                    .push(cts_frame(our_sa, peer, prio, pgn, window, next_seq));
                Vec::new()
            }
        }
    }

    /// Handle an inbound CTS — we are the sender. Queue the granted window
    /// of TP.DT (a `num_packets` of `0` is "wait/hold": reset the T3 timer
    /// and send nothing).
    fn on_cts(&mut self, decoded: &DecodedId, data: &[u8], now: Instant) -> Vec<TpEvent> {
        let Ok(pgn) = Pgn::new(transported_pgn_from(data)) else {
            return Vec::new();
        };
        let key = (decoded.source_addr, pgn);
        let num_to_send = data[1];
        let next_seq = data[2];
        let burst = {
            let Some(s) = self.tx_conn.get_mut(&key) else {
                return Vec::new();
            };
            s.last_activity = now;
            if num_to_send == 0 {
                return Vec::new();
            }
            build_dt_burst(s, next_seq, num_to_send)
        };
        self.pending_out.extend(burst);
        Vec::new()
    }

    /// Handle an inbound EndOfMsgAck — we are the sender. The peer received
    /// every packet, so the outbound session completes and is freed.
    fn on_end_of_msg_ack(&mut self, decoded: &DecodedId, data: &[u8]) -> Vec<TpEvent> {
        let Ok(pgn) = Pgn::new(transported_pgn_from(data)) else {
            return Vec::new();
        };
        self.tx_conn.remove(&(decoded.source_addr, pgn));
        Vec::new()
    }

    /// Handle an inbound Conn_Abort — either side. Abort and free the
    /// matching inbound and/or outbound session, surfacing one
    /// [`TpEvent::Aborted`] per affected session (`REQ_0895`). No abort is
    /// re-emitted onto the wire (the peer already did).
    fn on_conn_abort(&mut self, decoded: &DecodedId, data: &[u8]) -> Vec<TpEvent> {
        let Ok(pgn) = Pgn::new(transported_pgn_from(data)) else {
            return Vec::new();
        };
        let reason = abort_reason_from_u8(data[1]);
        let key = (decoded.source_addr, pgn);
        let mut events = Vec::new();
        if let Some(s) = self.rx_conn.remove(&key) {
            events.push(TpEvent::Aborted {
                pgn: s.transported_pgn,
                source_addr: s.peer_sa,
                reason,
            });
        }
        if let Some(s) = self.tx_conn.remove(&key) {
            events.push(TpEvent::Aborted {
                pgn: s.transported_pgn,
                source_addr: s.dest_sa,
                reason,
            });
        }
        events
    }

    // ---- ETP (Extended Transport Protocol), #125 -------------------

    /// Begin an OUTBOUND ETP transfer: register the session and return the
    /// ETP.RTS frame to transmit. The transfer then advances as ETP.CTS
    /// frames arrive via [`Self::on_frame`] (each granting a burst, drained
    /// as one ETP.DPO + N ETP.DT frames via [`Self::take_outbound`]),
    /// completing on the peer's ETP.EndOfMsgAck.
    ///
    /// Mirrors [`Self::start_outbound_connection`] but for payloads above
    /// 1785 bytes, bounded by `max_etp_bytes` (`REQ_0894`/`REQ_0903`).
    ///
    /// # Errors
    ///
    /// [`TpError::EtpPayloadOutOfRange`] when `payload.len()` is below
    /// [`ETP_MIN_PAYLOAD`] (use BAM/RTS-CTS instead) or above the
    /// configured `max_etp_bytes`.
    pub fn start_outbound_etp(
        &mut self,
        transported_pgn: Pgn,
        priority: u8,
        our_sa: u8,
        dest_addr: u8,
        payload: &[u8],
        now: Instant,
    ) -> Result<Vec<TpOutFrame>, TpError> {
        let size = payload.len();
        if !(ETP_MIN_PAYLOAD..=self.max_etp_bytes).contains(&size) {
            return Err(TpError::EtpPayloadOutOfRange {
                len: size,
                max: self.max_etp_bytes,
            });
        }
        let total_packets = size.div_ceil(TP_DT_DATA_LEN) as u32;
        self.etp_tx.insert(
            (dest_addr, transported_pgn),
            OutboundEtpSession {
                transported_pgn,
                our_sa,
                dest_sa: dest_addr,
                priority,
                payload: payload.to_vec(),
                total_packets,
                last_activity: now,
            },
        );
        Ok(vec![etp_rts_frame(
            our_sa,
            dest_addr,
            priority,
            transported_pgn,
            size,
        )])
    }

    /// Dispatch an ETP.CM frame by its control byte (#125).
    fn on_etp_cm(&mut self, decoded: &DecodedId, data: &[u8], now: Instant) -> Vec<TpEvent> {
        if data.len() < 8 {
            return Vec::new();
        }
        match data[0] {
            ETP_RTS_CONTROL => self.on_etp_rts(decoded, data, now),
            ETP_CTS_CONTROL => self.on_etp_cts(decoded, data, now),
            ETP_DPO_CONTROL => self.on_etp_dpo(decoded, data, now),
            ETP_EOMA_CONTROL => self.on_etp_eoma(decoded, data),
            ETP_ABORT_CONTROL => self.on_etp_abort(decoded, data),
            _ => Vec::new(),
        }
    }

    /// Handle an inbound ETP.RTS — we are the receiver. Enforce the
    /// `max_etp_bytes` cap on the announced 32-bit size BEFORE allocating
    /// any reassembly buffer (`REQ_0894`/`REQ_0903`): an oversize announce
    /// emits an ETP.Abort(Resources) on the wire plus a local
    /// [`TpEvent::Aborted`] and creates no session. Otherwise open a
    /// reassembly session and grant the first ETP.CTS burst.
    fn on_etp_rts(&mut self, decoded: &DecodedId, data: &[u8], now: Instant) -> Vec<TpEvent> {
        let Some((our_sa, peer_sa, priority, total_size, pgn)) = parse_etp_rts(decoded, data)
        else {
            return Vec::new();
        };

        // REQ_0894 / REQ_0903: a session announcing more than the cap is
        // aborted with the J1939 connection-abort reason. Resources (2) is
        // the chosen reason — "system resources needed for another task" —
        // because the cap is a deliberate local resource bound, not a
        // timeout or a protocol-state error. NO buffer is allocated.
        if total_size > self.max_etp_bytes {
            self.pending_out.push(etp_abort_frame(
                our_sa,
                peer_sa,
                priority,
                pgn,
                TpAbortReason::Resources,
            ));
            return vec![TpEvent::Aborted {
                pgn,
                source_addr: peer_sa,
                reason: TpAbortReason::Resources,
            }];
        }

        let key = (peer_sa, pgn);
        // REQ_0896: refuse a new session past the concurrency cap.
        if !self.etp_rx.contains_key(&key) && self.total_inbound() >= self.max_sessions {
            self.pending_out.push(etp_abort_frame(
                our_sa,
                peer_sa,
                priority,
                pgn,
                TpAbortReason::Resources,
            ));
            return vec![TpEvent::SessionRefused {
                pgn,
                source_addr: peer_sa,
                reason: TpAbortReason::Resources,
            }];
        }

        let total_packets = total_size.div_ceil(TP_DT_DATA_LEN) as u32;
        let window = etp_grant(total_packets, self.cts_window);
        self.etp_rx.insert(
            key,
            InboundEtpSession {
                transported_pgn: pgn,
                peer_sa,
                our_sa,
                priority,
                total_size,
                total_packets,
                next_packet: 1,
                dpo_offset: 0,
                burst_remaining: 0,
                buf: vec![0u8; total_size],
                last_activity: now,
            },
        );
        self.pending_out
            .push(etp_cts_frame(our_sa, peer_sa, priority, pgn, window, 1));
        Vec::new()
    }

    /// Handle an inbound ETP.CTS — we are the sender. Emit one ETP.DPO
    /// (carrying the packet offset so the 1-byte ETP.DT sequence can
    /// address packets beyond 255) followed by the granted burst of
    /// ETP.DT frames. A `num_packets` of `0` is "wait/hold".
    fn on_etp_cts(&mut self, decoded: &DecodedId, data: &[u8], now: Instant) -> Vec<TpEvent> {
        let Ok(pgn) = Pgn::new(transported_pgn_from(data)) else {
            return Vec::new();
        };
        let key = (decoded.source_addr, pgn);
        let num = data[1];
        let next_pkt = u24_from(&data[2..5]);
        let burst = {
            let Some(s) = self.etp_tx.get_mut(&key) else {
                return Vec::new();
            };
            s.last_activity = now;
            if num == 0 || next_pkt == 0 {
                return Vec::new();
            }
            build_etp_burst(s, next_pkt, u32::from(num))
        };
        self.pending_out.extend(burst);
        Vec::new()
    }

    /// Handle an inbound ETP.DPO — we are the receiver. Record the burst
    /// packet offset and count so the following ETP.DT packets map to the
    /// correct absolute packet numbers / byte offsets.
    fn on_etp_dpo(&mut self, decoded: &DecodedId, data: &[u8], now: Instant) -> Vec<TpEvent> {
        let Ok(pgn) = Pgn::new(transported_pgn_from(data)) else {
            return Vec::new();
        };
        let key = (decoded.source_addr, pgn);
        if let Some(s) = self.etp_rx.get_mut(&key) {
            s.dpo_offset = u24_from(&data[2..5]);
            s.burst_remaining = u32::from(data[1]);
            s.last_activity = now;
        }
        Vec::new()
    }

    /// Handle an inbound ETP.DT — we are the receiver. The absolute packet
    /// number is `dpo_offset + seq_no`; the byte offset into the
    /// reassembly buffer is `(absolute - 1) * 7`. On the final packet emit
    /// ETP.EndOfMsgAck and complete; on burst exhaustion grant the next
    /// ETP.CTS.
    fn on_etp_dt(&mut self, decoded: &DecodedId, data: &[u8], now: Instant) -> Vec<TpEvent> {
        if data.is_empty() {
            return Vec::new();
        }
        let seq = u32::from(data[0]);
        let Some(key) = self
            .etp_rx
            .keys()
            .find(|(sa, _)| *sa == decoded.source_addr)
            .copied()
        else {
            return Vec::new();
        };

        /// Post-append action, computed while the session is borrowed.
        enum Act {
            Drop,
            Wait,
            Complete,
            NextBurst { window: u8, next_pkt: u32 },
        }

        let act = {
            let s = self.etp_rx.get_mut(&key).expect("key just located");
            let absolute = s.dpo_offset.saturating_add(seq);
            if absolute != s.next_packet || absolute == 0 || absolute > s.total_packets {
                Act::Drop
            } else {
                let byte_off = (absolute as usize - 1) * TP_DT_DATA_LEN;
                let remaining = s.total_size.saturating_sub(byte_off);
                let available = data.len().saturating_sub(1);
                let take = remaining.min(TP_DT_DATA_LEN).min(available);
                s.buf[byte_off..byte_off + take].copy_from_slice(&data[1..=take]);
                s.next_packet += 1;
                s.burst_remaining = s.burst_remaining.saturating_sub(1);
                s.last_activity = now;
                if s.next_packet > s.total_packets {
                    Act::Complete
                } else if s.burst_remaining == 0 {
                    let remaining_packets = s.total_packets - (s.next_packet - 1);
                    Act::NextBurst {
                        window: etp_grant(remaining_packets, self.cts_window),
                        next_pkt: s.next_packet,
                    }
                } else {
                    Act::Wait
                }
            }
        };

        match act {
            Act::Drop | Act::Wait => Vec::new(),
            Act::Complete => {
                let done = self.etp_rx.remove(&key).expect("key just located");
                self.pending_out.push(etp_eoma_frame(
                    done.our_sa,
                    done.peer_sa,
                    done.priority,
                    done.transported_pgn,
                    done.total_size,
                ));
                vec![TpEvent::Completed {
                    pgn: done.transported_pgn,
                    source_addr: done.peer_sa,
                    payload: done.buf,
                }]
            }
            Act::NextBurst { window, next_pkt } => {
                let (our_sa, peer, prio, pgn) = {
                    let s = self.etp_rx.get(&key).expect("key just located");
                    (s.our_sa, s.peer_sa, s.priority, s.transported_pgn)
                };
                self.pending_out
                    .push(etp_cts_frame(our_sa, peer, prio, pgn, window, next_pkt));
                Vec::new()
            }
        }
    }

    /// Handle an inbound ETP.EndOfMsgAck — we are the sender. The peer
    /// received every packet, so the outbound session completes.
    fn on_etp_eoma(&mut self, decoded: &DecodedId, data: &[u8]) -> Vec<TpEvent> {
        let Ok(pgn) = Pgn::new(transported_pgn_from(data)) else {
            return Vec::new();
        };
        self.etp_tx.remove(&(decoded.source_addr, pgn));
        Vec::new()
    }

    /// Handle an inbound ETP.Abort — either side. Free the matching
    /// inbound and/or outbound ETP session, surfacing one
    /// [`TpEvent::Aborted`] per affected session (`REQ_0895`).
    fn on_etp_abort(&mut self, decoded: &DecodedId, data: &[u8]) -> Vec<TpEvent> {
        let Ok(pgn) = Pgn::new(transported_pgn_from(data)) else {
            return Vec::new();
        };
        let reason = abort_reason_from_u8(data[1]);
        let key = (decoded.source_addr, pgn);
        let mut events = Vec::new();
        if let Some(s) = self.etp_rx.remove(&key) {
            events.push(TpEvent::Aborted {
                pgn: s.transported_pgn,
                source_addr: s.peer_sa,
                reason,
            });
        }
        if let Some(s) = self.etp_tx.remove(&key) {
            events.push(TpEvent::Aborted {
                pgn: s.transported_pgn,
                source_addr: s.dest_sa,
                reason,
            });
        }
        events
    }

    /// Poll every in-progress session for a T1 timeout (`REQ_0895`). A
    /// session whose gap since its last accepted frame exceeds
    /// [`TpTimers::t1`] is aborted and removed; the dispatcher turns the
    /// returned [`TpEvent::Aborted`] into a `HealthEvent`.
    #[must_use]
    pub fn poll_timeouts(&mut self, now: Instant) -> Vec<TpEvent> {
        let t1 = self.timers.t1;
        let expired: Vec<(u8, Pgn)> = self
            .sessions
            .iter()
            .filter(|(_, s)| now.saturating_duration_since(s.last_activity) > t1)
            .map(|(k, _)| *k)
            .collect();
        let mut events = Vec::with_capacity(expired.len());
        for key in expired {
            let s = self.sessions.remove(&key).expect("expired key present");
            events.push(TpEvent::Aborted {
                pgn: s.transported_pgn,
                source_addr: s.source_addr,
                reason: TpAbortReason::Timeout,
            });
        }

        // Connection-mode receive side (#124): T2 bounds the wait for the
        // next TP.DT after a CTS. On timeout, emit a Conn_Abort onto the
        // wire and surface the abort locally (`REQ_0895`).
        let t2 = self.timers.t2;
        let rx_expired: Vec<(u8, Pgn)> = self
            .rx_conn
            .iter()
            .filter(|(_, s)| now.saturating_duration_since(s.last_activity) > t2)
            .map(|(k, _)| *k)
            .collect();
        for key in rx_expired {
            let s = self.rx_conn.remove(&key).expect("expired key present");
            self.pending_out.push(conn_abort_frame(
                s.our_sa,
                s.peer_sa,
                s.priority,
                s.transported_pgn,
                TpAbortReason::Timeout,
            ));
            events.push(TpEvent::Aborted {
                pgn: s.transported_pgn,
                source_addr: s.peer_sa,
                reason: TpAbortReason::Timeout,
            });
        }

        // Connection-mode send side (#124): T3 bounds the wait for the next
        // CTS after sending a burst / the RTS. On timeout, emit a
        // Conn_Abort and surface the abort locally.
        let t3 = self.timers.t3;
        let tx_expired: Vec<(u8, Pgn)> = self
            .tx_conn
            .iter()
            .filter(|(_, s)| now.saturating_duration_since(s.last_activity) > t3)
            .map(|(k, _)| *k)
            .collect();
        for key in tx_expired {
            let s = self.tx_conn.remove(&key).expect("expired key present");
            self.pending_out.push(conn_abort_frame(
                s.our_sa,
                s.dest_sa,
                s.priority,
                s.transported_pgn,
                TpAbortReason::Timeout,
            ));
            events.push(TpEvent::Aborted {
                pgn: s.transported_pgn,
                source_addr: s.dest_sa,
                reason: TpAbortReason::Timeout,
            });
        }

        // ETP timeouts (#125): T2 (receive) / T3 (send), mirroring the
        // connection-mode sweep above. Extracted to keep this fn bounded.
        events.extend(self.poll_etp_timeouts(now, t2, t3));

        events
    }

    /// Sweep the ETP receive (T2) and send (T3) sessions for timeouts,
    /// emitting an ETP.Abort onto the wire and a local
    /// [`TpEvent::Aborted`] for each expired session (#125, `REQ_0895`).
    fn poll_etp_timeouts(&mut self, now: Instant, t2: Duration, t3: Duration) -> Vec<TpEvent> {
        let mut events = Vec::new();

        let rx_expired: Vec<(u8, Pgn)> = self
            .etp_rx
            .iter()
            .filter(|(_, s)| now.saturating_duration_since(s.last_activity) > t2)
            .map(|(k, _)| *k)
            .collect();
        for key in rx_expired {
            let s = self.etp_rx.remove(&key).expect("expired key present");
            self.pending_out.push(etp_abort_frame(
                s.our_sa,
                s.peer_sa,
                s.priority,
                s.transported_pgn,
                TpAbortReason::Timeout,
            ));
            events.push(TpEvent::Aborted {
                pgn: s.transported_pgn,
                source_addr: s.peer_sa,
                reason: TpAbortReason::Timeout,
            });
        }

        let tx_expired: Vec<(u8, Pgn)> = self
            .etp_tx
            .iter()
            .filter(|(_, s)| now.saturating_duration_since(s.last_activity) > t3)
            .map(|(k, _)| *k)
            .collect();
        for key in tx_expired {
            let s = self.etp_tx.remove(&key).expect("expired key present");
            self.pending_out.push(etp_abort_frame(
                s.our_sa,
                s.dest_sa,
                s.priority,
                s.transported_pgn,
                TpAbortReason::Timeout,
            ));
            events.push(TpEvent::Aborted {
                pgn: s.transported_pgn,
                source_addr: s.dest_sa,
                reason: TpAbortReason::Timeout,
            });
        }

        events
    }

    /// Segment a whole outbound message into BAM frames: one TP.CM(BAM)
    /// announce followed by `ceil(len/7)` TP.DT frames (`REQ_0892`). The
    /// final TP.DT is padded with `0xFF`.
    ///
    /// `transported_pgn` is the data PGN the message is sent as;
    /// `priority` / `source_addr` are applied to the encoded TP.CM/TP.DT
    /// identifiers. BAM is broadcast, so every frame targets the global
    /// address `0xFF`.
    ///
    /// # Errors
    ///
    /// [`TpError::PayloadOutOfRange`] when `payload.len()` is outside
    /// `9..=1785`.
    pub fn segment_outbound(
        &self,
        transported_pgn: Pgn,
        priority: u8,
        source_addr: u8,
        payload: &[u8],
    ) -> Result<Vec<TpOutFrame>, TpError> {
        let size = payload.len();
        if !(TP_MIN_PAYLOAD..=TP_MAX_PAYLOAD).contains(&size) {
            return Err(TpError::PayloadOutOfRange(size));
        }
        let num_packets = size.div_ceil(TP_DT_DATA_LEN);
        let pgn_value = transported_pgn.value();
        let mut frames = Vec::with_capacity(num_packets + 1);

        let size_lo = (size & 0xFF) as u8;
        let size_hi = ((size >> 8) & 0xFF) as u8;
        let cm = [
            BAM_CONTROL,
            size_lo,
            size_hi,
            num_packets as u8,
            0xFF,
            (pgn_value & 0xFF) as u8,
            ((pgn_value >> 8) & 0xFF) as u8,
            ((pgn_value >> 16) & 0xFF) as u8,
        ];
        frames.push(TpOutFrame {
            wire_pgn: cm_pgn(),
            priority,
            source_addr,
            dest_addr: Some(GLOBAL_ADDRESS),
            data: cm,
        });

        for i in 0..num_packets {
            let start = i * TP_DT_DATA_LEN;
            let end = (start + TP_DT_DATA_LEN).min(size);
            let mut data = [0xFFu8; 8];
            data[0] = (i + 1) as u8;
            data[1..=(end - start)].copy_from_slice(&payload[start..end]);
            frames.push(TpOutFrame {
                wire_pgn: dt_pgn(),
                priority,
                source_addr,
                dest_addr: Some(GLOBAL_ADDRESS),
                data,
            });
        }
        Ok(frames)
    }
}

/// The validated TP.CM PGN newtype.
#[must_use]
pub fn cm_pgn() -> Pgn {
    Pgn::new(TP_CM_PGN).expect("TP_CM_PGN is a valid 18-bit PGN")
}

/// The validated TP.DT PGN newtype.
#[must_use]
pub fn dt_pgn() -> Pgn {
    Pgn::new(TP_DT_PGN).expect("TP_DT_PGN is a valid 18-bit PGN")
}

/// Extract the transported PGN (bytes 5..7, little-endian) from a TP.CM
/// frame's data.
fn transported_pgn_from(data: &[u8]) -> u32 {
    u32::from(data[5]) | (u32::from(data[6]) << 8) | (u32::from(data[7]) << 16)
}

/// Validated fields parsed from a connection-mode TP.CM(RTS) frame.
struct ConnRtsParams {
    our_sa: u8,
    peer_sa: u8,
    priority: u8,
    total_size: usize,
    total_packets: u8,
    sender_max_per_cts: u8,
    pgn: Pgn,
}

/// Parse + validate a connection-mode RTS. Returns `None` (one exit) for
/// any malformed or non-connection-mode frame, collapsing the guards that
/// would otherwise be separate early returns in `on_rts`.
fn parse_conn_rts(decoded: &DecodedId, data: &[u8]) -> Option<ConnRtsParams> {
    // Connection mode is destination-specific; a global DA is BAM.
    let our_sa = decoded.dest_addr.filter(|&sa| sa != GLOBAL_ADDRESS)?;
    let total_size = usize::from(u16::from_le_bytes([data[1], data[2]]));
    let total_packets = data[3];
    let pgn = Pgn::new(transported_pgn_from(data)).ok()?;
    let well_formed = (TP_MIN_PAYLOAD..=TP_MAX_PAYLOAD).contains(&total_size)
        && usize::from(total_packets) == total_size.div_ceil(TP_DT_DATA_LEN);
    well_formed.then_some(ConnRtsParams {
        our_sa,
        peer_sa: decoded.source_addr,
        priority: decoded.priority,
        total_size,
        total_packets,
        sender_max_per_cts: data[4],
        pgn,
    })
}

/// Parse + validate an ETP.CM(RTS). Returns `Some((our_sa, peer_sa,
/// priority, total_size, pgn))` for a well-formed announcement, else
/// `None` — collapsing `on_etp_rts`'s validation guards into one exit.
fn parse_etp_rts(decoded: &DecodedId, data: &[u8]) -> Option<(u8, u8, u8, usize, Pgn)> {
    // ETP is destination-specific; a global DA is not an ETP session.
    let our_sa = decoded.dest_addr.filter(|&sa| sa != GLOBAL_ADDRESS)?;
    let total_size = u32::from_le_bytes([data[1], data[2], data[3], data[4]]) as usize;
    let pgn = Pgn::new(transported_pgn_from(data)).ok()?;
    (ETP_MIN_PAYLOAD..=ETP_PROTOCOL_MAX)
        .contains(&total_size)
        .then_some((
            our_sa,
            decoded.source_addr,
            decoded.priority,
            total_size,
            pgn,
        ))
}

/// Map a J1939-21 abort reason byte to a [`TpAbortReason`] (unknown codes
/// fold to [`TpAbortReason::Other`]).
const fn abort_reason_from_u8(code: u8) -> TpAbortReason {
    match code {
        1 => TpAbortReason::AlreadyInSession,
        2 => TpAbortReason::Resources,
        3 => TpAbortReason::Timeout,
        _ => TpAbortReason::Other,
    }
}

/// The CTS window to grant: the smaller of the packets still outstanding,
/// our configured window, and (when limited) the sender's
/// `max_packets_per_cts`. Always at least `1`.
fn grant_window(remaining_packets: u8, cts_window: u8, sender_max_per_cts: u8) -> u8 {
    let mut window = remaining_packets.min(cts_window.max(1));
    if sender_max_per_cts != CTS_NO_LIMIT {
        window = window.min(sender_max_per_cts.max(1));
    }
    window.max(1)
}

/// Build the three transported-PGN bytes for a TP.CM frame.
const fn pgn_bytes(pgn: Pgn) -> [u8; 3] {
    let v = pgn.value();
    [
        (v & 0xFF) as u8,
        ((v >> 8) & 0xFF) as u8,
        ((v >> 16) & 0xFF) as u8,
    ]
}

/// Build an RTS (Request To Send) TP.CM frame (#124).
fn rts_frame(
    our_sa: u8,
    dest_sa: u8,
    priority: u8,
    pgn: Pgn,
    size: usize,
    total_packets: u8,
    max_per_cts: u8,
) -> TpOutFrame {
    let p = pgn_bytes(pgn);
    TpOutFrame {
        wire_pgn: cm_pgn(),
        priority,
        source_addr: our_sa,
        dest_addr: Some(dest_sa),
        data: [
            RTS_CONTROL,
            (size & 0xFF) as u8,
            ((size >> 8) & 0xFF) as u8,
            total_packets,
            max_per_cts,
            p[0],
            p[1],
            p[2],
        ],
    }
}

/// Build a CTS (Clear To Send) TP.CM frame granting `num_packets` starting
/// at `next_seq` (#124).
fn cts_frame(
    our_sa: u8,
    dest_sa: u8,
    priority: u8,
    pgn: Pgn,
    num_packets: u8,
    next_seq: u8,
) -> TpOutFrame {
    let p = pgn_bytes(pgn);
    TpOutFrame {
        wire_pgn: cm_pgn(),
        priority,
        source_addr: our_sa,
        dest_addr: Some(dest_sa),
        data: [
            CTS_CONTROL,
            num_packets,
            next_seq,
            0xFF,
            0xFF,
            p[0],
            p[1],
            p[2],
        ],
    }
}

/// Build an EndOfMsgAck TP.CM frame (#124).
fn end_of_msg_ack_frame(
    our_sa: u8,
    dest_sa: u8,
    priority: u8,
    pgn: Pgn,
    size: usize,
    total_packets: u8,
) -> TpOutFrame {
    let p = pgn_bytes(pgn);
    TpOutFrame {
        wire_pgn: cm_pgn(),
        priority,
        source_addr: our_sa,
        dest_addr: Some(dest_sa),
        data: [
            EOMA_CONTROL,
            (size & 0xFF) as u8,
            ((size >> 8) & 0xFF) as u8,
            total_packets,
            0xFF,
            p[0],
            p[1],
            p[2],
        ],
    }
}

/// Build a Conn_Abort TP.CM frame carrying `reason` (#124).
fn conn_abort_frame(
    our_sa: u8,
    dest_sa: u8,
    priority: u8,
    pgn: Pgn,
    reason: TpAbortReason,
) -> TpOutFrame {
    let p = pgn_bytes(pgn);
    TpOutFrame {
        wire_pgn: cm_pgn(),
        priority,
        source_addr: our_sa,
        dest_addr: Some(dest_sa),
        data: [
            CONN_ABORT_CONTROL,
            reason.as_u8(),
            0xFF,
            0xFF,
            0xFF,
            p[0],
            p[1],
            p[2],
        ],
    }
}

/// Build the TP.DT burst for a connection-mode send: `count` packets
/// starting at `start_seq`, each carrying up to 7 payload bytes (the final
/// packet padded with `0xFF`). Sequence numbers outside `1..=total_packets`
/// are skipped (#124).
fn build_dt_burst(s: &OutboundConnSession, start_seq: u8, count: u8) -> Vec<TpOutFrame> {
    let mut frames = Vec::with_capacity(usize::from(count));
    for i in 0..count {
        let seq = start_seq.wrapping_add(i);
        if seq < 1 || usize::from(seq) > usize::from(s.total_packets) {
            break;
        }
        let start = usize::from(seq - 1) * TP_DT_DATA_LEN;
        let end = (start + TP_DT_DATA_LEN).min(s.payload.len());
        let mut data = [0xFFu8; 8];
        data[0] = seq;
        data[1..=(end - start)].copy_from_slice(&s.payload[start..end]);
        frames.push(TpOutFrame {
            wire_pgn: dt_pgn(),
            priority: s.priority,
            source_addr: s.our_sa,
            dest_addr: Some(s.dest_sa),
            data,
        });
    }
    frames
}

/// The validated ETP.CM PGN newtype.
#[must_use]
pub fn etp_cm_pgn() -> Pgn {
    Pgn::new(ETP_CM_PGN).expect("ETP_CM_PGN is a valid 18-bit PGN")
}

/// The validated ETP.DT PGN newtype.
#[must_use]
pub fn etp_dt_pgn() -> Pgn {
    Pgn::new(ETP_DT_PGN).expect("ETP_DT_PGN is a valid 18-bit PGN")
}

/// Read a 24-bit little-endian value from a 3-byte slice (ETP.CTS next
/// packet / ETP.DPO offset).
fn u24_from(bytes: &[u8]) -> u32 {
    u32::from(bytes[0]) | (u32::from(bytes[1]) << 8) | (u32::from(bytes[2]) << 16)
}

/// Split a 24-bit value into its three little-endian bytes.
const fn u24_bytes(v: u32) -> [u8; 3] {
    [
        (v & 0xFF) as u8,
        ((v >> 8) & 0xFF) as u8,
        ((v >> 16) & 0xFF) as u8,
    ]
}

/// The ETP burst to grant: the smaller of the packets still outstanding
/// and our configured window. Always at least `1`. ETP.RTS carries no
/// sender-side per-burst limit (unlike RTS/CTS), so only the receiver
/// window bounds the grant.
fn etp_grant(remaining_packets: u32, cts_window: u8) -> u8 {
    let window = u32::from(cts_window.max(1));
    remaining_packets.min(window).max(1) as u8
}

/// Build an ETP.RTS frame carrying the 32-bit total size (#125).
fn etp_rts_frame(our_sa: u8, dest_sa: u8, priority: u8, pgn: Pgn, size: usize) -> TpOutFrame {
    let p = pgn_bytes(pgn);
    let s = (size as u32).to_le_bytes();
    TpOutFrame {
        wire_pgn: etp_cm_pgn(),
        priority,
        source_addr: our_sa,
        dest_addr: Some(dest_sa),
        data: [ETP_RTS_CONTROL, s[0], s[1], s[2], s[3], p[0], p[1], p[2]],
    }
}

/// Build an ETP.CTS frame granting `num_packets` starting at the 24-bit
/// `next_pkt` (1-based, may exceed 255) (#125).
fn etp_cts_frame(
    our_sa: u8,
    dest_sa: u8,
    priority: u8,
    pgn: Pgn,
    num_packets: u8,
    next_pkt: u32,
) -> TpOutFrame {
    let p = pgn_bytes(pgn);
    let n = u24_bytes(next_pkt);
    TpOutFrame {
        wire_pgn: etp_cm_pgn(),
        priority,
        source_addr: our_sa,
        dest_addr: Some(dest_sa),
        data: [
            ETP_CTS_CONTROL,
            num_packets,
            n[0],
            n[1],
            n[2],
            p[0],
            p[1],
            p[2],
        ],
    }
}

/// Build an ETP.DPO frame announcing a burst of `num_packets` at the
/// 24-bit `offset` (#125).
fn etp_dpo_frame(
    our_sa: u8,
    dest_sa: u8,
    priority: u8,
    pgn: Pgn,
    num_packets: u8,
    offset: u32,
) -> TpOutFrame {
    let p = pgn_bytes(pgn);
    let o = u24_bytes(offset);
    TpOutFrame {
        wire_pgn: etp_cm_pgn(),
        priority,
        source_addr: our_sa,
        dest_addr: Some(dest_sa),
        data: [
            ETP_DPO_CONTROL,
            num_packets,
            o[0],
            o[1],
            o[2],
            p[0],
            p[1],
            p[2],
        ],
    }
}

/// Build an ETP.EndOfMsgAck frame carrying the 32-bit total size (#125).
fn etp_eoma_frame(our_sa: u8, dest_sa: u8, priority: u8, pgn: Pgn, size: usize) -> TpOutFrame {
    let p = pgn_bytes(pgn);
    let s = (size as u32).to_le_bytes();
    TpOutFrame {
        wire_pgn: etp_cm_pgn(),
        priority,
        source_addr: our_sa,
        dest_addr: Some(dest_sa),
        data: [ETP_EOMA_CONTROL, s[0], s[1], s[2], s[3], p[0], p[1], p[2]],
    }
}

/// Build an ETP.Abort frame carrying `reason` on the ETP.CM PGN (#125).
fn etp_abort_frame(
    our_sa: u8,
    dest_sa: u8,
    priority: u8,
    pgn: Pgn,
    reason: TpAbortReason,
) -> TpOutFrame {
    let p = pgn_bytes(pgn);
    TpOutFrame {
        wire_pgn: etp_cm_pgn(),
        priority,
        source_addr: our_sa,
        dest_addr: Some(dest_sa),
        data: [
            ETP_ABORT_CONTROL,
            reason.as_u8(),
            0xFF,
            0xFF,
            0xFF,
            p[0],
            p[1],
            p[2],
        ],
    }
}

/// Build one ETP.DT burst for a sender: an ETP.DPO (offset = `next_pkt -
/// 1`) followed by `count` ETP.DT packets. ETP.DT sequence numbers reset
/// to `1..=count` each burst; the absolute packet (and thus the byte
/// offset `(absolute - 1) * 7`) is recovered by the receiver from the DPO
/// offset. Packets beyond `total_packets` are skipped (#125).
fn build_etp_burst(s: &OutboundEtpSession, next_pkt: u32, count: u32) -> Vec<TpOutFrame> {
    let offset = next_pkt - 1;
    let last = next_pkt
        .saturating_add(count)
        .saturating_sub(1)
        .min(s.total_packets);
    let effective = last.saturating_sub(next_pkt).saturating_add(1);
    let mut frames = Vec::with_capacity(effective as usize + 1);
    frames.push(etp_dpo_frame(
        s.our_sa,
        s.dest_sa,
        s.priority,
        s.transported_pgn,
        effective as u8,
        offset,
    ));
    for i in 0..effective {
        let absolute = next_pkt + i;
        let seq = (i + 1) as u8; // burst-relative, 1-based
        let start = (absolute as usize - 1) * TP_DT_DATA_LEN;
        let end = (start + TP_DT_DATA_LEN).min(s.payload.len());
        let mut data = [0xFFu8; 8];
        data[0] = seq;
        data[1..=(end - start)].copy_from_slice(&s.payload[start..end]);
        frames.push(TpOutFrame {
            wire_pgn: etp_dt_pgn(),
            priority: s.priority,
            source_addr: s.our_sa,
            dest_addr: Some(s.dest_sa),
            data,
        });
    }
    frames
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::decode_extended_id;
    use crate::decode::encode_extended_id;

    fn pgn(v: u32) -> Pgn {
        Pgn::new(v).unwrap()
    }

    fn decoded(wire_pgn: u32, sa: u8) -> DecodedId {
        decode_extended_id(encode_extended_id(
            pgn(wire_pgn),
            7,
            sa,
            Some(GLOBAL_ADDRESS),
        ))
    }

    #[test]
    fn timers_have_standard_defaults() {
        let t = TpTimers::default();
        assert_eq!(t.tr, Duration::from_millis(200));
        assert_eq!(t.th, Duration::from_millis(500));
        assert_eq!(t.t1, Duration::from_millis(750));
        assert_eq!(t.t2, Duration::from_millis(1250));
        assert_eq!(t.t3, Duration::from_millis(1250));
        assert_eq!(t.t4, Duration::from_millis(1050));
    }

    #[test]
    fn segment_outbound_round_trips_through_on_frame() {
        let engine = TpEngine::new(TpTimers::default(), 8);
        let payload: Vec<u8> = (0..50u8).collect();
        let frames = engine
            .segment_outbound(pgn(65260), 7, 0x11, &payload)
            .unwrap();
        // CM + ceil(50/7)=8 DT frames.
        assert_eq!(frames.len(), 1 + 8);
        assert_eq!(frames[0].data[0], BAM_CONTROL);
        assert_eq!(frames[0].wire_pgn.value(), TP_CM_PGN);
        assert_eq!(frames[1].wire_pgn.value(), TP_DT_PGN);

        let mut rx = TpEngine::new(TpTimers::default(), 8);
        let now = Instant::now();
        let mut completed = None;
        for f in &frames {
            let dec = decode_extended_id(encode_extended_id(
                f.wire_pgn,
                f.priority,
                f.source_addr,
                f.dest_addr,
            ));
            for ev in rx.on_frame(&dec, &f.data, now) {
                if let TpEvent::Completed { payload, pgn, .. } = ev {
                    assert_eq!(pgn.value(), 65260);
                    completed = Some(payload);
                }
            }
        }
        assert_eq!(completed.unwrap(), payload);
        assert_eq!(rx.active_inbound_sessions(), 0);
    }

    #[test]
    fn payload_out_of_range_is_rejected() {
        let engine = TpEngine::new(TpTimers::default(), 8);
        assert_eq!(
            engine.segment_outbound(pgn(65260), 7, 0x11, &[0; 8]),
            Err(TpError::PayloadOutOfRange(8))
        );
        assert_eq!(
            engine.segment_outbound(pgn(65260), 7, 0x11, &vec![0; 1786]),
            Err(TpError::PayloadOutOfRange(1786))
        );
    }

    #[test]
    fn t1_gap_aborts_with_timeout_reason() {
        let mut rx = TpEngine::new(TpTimers::default(), 8);
        let t0 = Instant::now();
        // Open a 50-byte BAM and accept the first DT, then withhold.
        let frames = rx
            .segment_outbound(pgn(65260), 7, 0x11, &(0..50u8).collect::<Vec<_>>())
            .unwrap();
        let cm = decode_extended_id(encode_extended_id(
            frames[0].wire_pgn,
            7,
            frames[0].source_addr,
            frames[0].dest_addr,
        ));
        assert!(rx.on_frame(&cm, &frames[0].data, t0).is_empty());
        let dt = decode_extended_id(encode_extended_id(
            frames[1].wire_pgn,
            7,
            frames[1].source_addr,
            frames[1].dest_addr,
        ));
        assert!(rx.on_frame(&dt, &frames[1].data, t0).is_empty());
        assert_eq!(rx.active_inbound_sessions(), 1);

        // No timeout just before T1.
        assert!(rx.poll_timeouts(t0 + Duration::from_millis(749)).is_empty());
        // Aborted just after T1.
        let events = rx.poll_timeouts(t0 + Duration::from_millis(751));
        assert_eq!(events.len(), 1);
        match &events[0] {
            TpEvent::Aborted { reason, .. } => assert_eq!(*reason, TpAbortReason::Timeout),
            other => panic!("expected Aborted, got {other:?}"),
        }
        assert_eq!(rx.active_inbound_sessions(), 0);
    }

    #[test]
    fn rts_opens_inbound_connection_and_grants_first_cts() {
        let mut rx = TpEngine::new(TpTimers::default(), 8).with_cts_window(4);
        let now = Instant::now();
        // 100-byte payload → 15 packets; window 4 ⇒ multiple CTS rounds.
        let payload: Vec<u8> = (0..100u8).collect();
        let mut tx = TpEngine::new(TpTimers::default(), 8);
        let rts = tx
            .start_outbound_connection(pgn(65260), 7, 0x22, 0x11, &payload, now)
            .unwrap();
        let dec = decode_extended_id(encode_extended_id(
            rts[0].wire_pgn,
            rts[0].priority,
            rts[0].source_addr,
            rts[0].dest_addr,
        ));
        assert!(rx.on_frame(&dec, &rts[0].data, now).is_empty());
        let cts = rx.take_outbound();
        assert_eq!(cts.len(), 1);
        assert_eq!(cts[0].data[0], CTS_CONTROL);
        assert_eq!(cts[0].data[1], 4, "granted window is the configured 4");
        assert_eq!(cts[0].data[2], 1, "first window starts at packet 1");
        assert_eq!(rx.active_inbound_connections(), 1);
    }

    #[test]
    fn receiver_t2_timeout_aborts_with_conn_abort_frame() {
        let mut rx = TpEngine::new(TpTimers::default(), 8).with_cts_window(4);
        let t0 = Instant::now();
        let payload: Vec<u8> = (0..100u8).collect();
        let mut tx = TpEngine::new(TpTimers::default(), 8);
        let rts = tx
            .start_outbound_connection(pgn(65260), 7, 0x22, 0x11, &payload, t0)
            .unwrap();
        let dec = decode_extended_id(encode_extended_id(
            rts[0].wire_pgn,
            rts[0].priority,
            rts[0].source_addr,
            rts[0].dest_addr,
        ));
        let _ = rx.on_frame(&dec, &rts[0].data, t0);
        let _ = rx.take_outbound(); // first CTS

        // No TP.DT arrives; T2 (1250ms) fires.
        assert!(
            rx.poll_timeouts(t0 + Duration::from_millis(1249))
                .is_empty()
        );
        let events = rx.poll_timeouts(t0 + Duration::from_millis(1251));
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0],
            TpEvent::Aborted {
                reason: TpAbortReason::Timeout,
                ..
            }
        ));
        let aborts = rx.take_outbound();
        assert_eq!(aborts.len(), 1, "a Conn_Abort frame was emitted on the bus");
        assert_eq!(aborts[0].data[0], CONN_ABORT_CONTROL);
        assert_eq!(aborts[0].data[1], TpAbortReason::Timeout.as_u8());
        assert_eq!(rx.active_inbound_connections(), 0);
    }

    #[test]
    fn sender_t3_timeout_aborts_when_cts_never_arrives() {
        let mut tx = TpEngine::new(TpTimers::default(), 8);
        let t0 = Instant::now();
        let payload: Vec<u8> = (0..50u8).collect();
        let _ = tx
            .start_outbound_connection(pgn(65260), 7, 0x22, 0x11, &payload, t0)
            .unwrap();
        assert_eq!(tx.active_outbound_connections(), 1);
        // No CTS arrives; T3 (1250ms) fires.
        let events = tx.poll_timeouts(t0 + Duration::from_millis(1251));
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0],
            TpEvent::Aborted {
                reason: TpAbortReason::Timeout,
                ..
            }
        ));
        let aborts = tx.take_outbound();
        assert_eq!(aborts.len(), 1);
        assert_eq!(aborts[0].data[0], CONN_ABORT_CONTROL);
        assert_eq!(tx.active_outbound_connections(), 0);
    }

    #[test]
    fn session_cap_refuses_excess_with_resources_reason() {
        let mut rx = TpEngine::new(TpTimers::default(), 2);
        let now = Instant::now();
        // Three distinct sources each open a BAM for the same PGN.
        let mut refused = 0;
        for sa in [0x10u8, 0x11, 0x12] {
            let frames = rx
                .segment_outbound(pgn(65260), 7, sa, &(0..20u8).collect::<Vec<_>>())
                .unwrap();
            let cm = decoded(frames[0].wire_pgn.value(), sa);
            for ev in rx.on_frame(&cm, &frames[0].data, now) {
                if let TpEvent::SessionRefused { reason, .. } = ev {
                    assert_eq!(reason, TpAbortReason::Resources);
                    refused += 1;
                }
            }
        }
        assert_eq!(refused, 1, "exactly the third session is refused");
        assert_eq!(rx.active_inbound_sessions(), 2, "no unbounded allocation");
    }
}
