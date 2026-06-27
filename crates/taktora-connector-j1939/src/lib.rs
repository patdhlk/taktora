//! J1939 reference connector — `BB_0098` / `FEAT_0098`.
//!
//! The foundational tracer bullet for the J1939 connector: 29-bit
//! extended-id decode and **single-frame** PGN routing over the CAN
//! connector's driver layer. Four follow-on issues build directly on
//! the seams scaffolded here — #123 (BAM transport), #124 (RTS/CTS),
//! #125 (ETP), and #126 (address-claim).
//!
//! Layer-1 (always available, portable across Linux / macOS / Windows):
//!
//! * [`decode`] — pure 29-bit id decode / encode (`REQ_0890`). The
//!   load-bearing logic every later issue reuses.
//! * [`routing`] — [`Pgn`] newtype, [`TransportClass`], and
//!   [`J1939Routing`] with the PGN/SA/DA demux predicate (`REQ_0890`,
//!   `REQ_0891`).
//! * [`options::J1939ConnectorOptions`] — typed builder
//!   (`BB_0099`).
//! * [`registry::J1939Registry`] — routing registry (`BB_0100`).
//! * [`gateway::J1939Gateway`] — owns the per-gateway tokio runtime
//!   (`BB_0100`).
//! * [`dispatcher`] — single-frame RX/TX loops with documented TP seams
//!   for #123–#125 (`BB_0100`).
//! * [`mock::MockJ1939Interface`] — layer-1 harness over
//!   `taktora_connector_can::MockCanInterface` (`BB_0103`, `REQ_0899`).
//! * [`connector::J1939Connector`] — implements
//!   [`taktora_connector_host::Connector`].
//!
//! Layer-2 (Linux-only): real CAN I/O is reached through
//! `taktora-connector-can`'s `socketcan-integration` feature, exposed
//! here as the passthrough `socketcan-integration` cargo feature
//! (`REQ_0899`). `MockJ1939Interface` ships ungated.
//!
//! The health monitor, bounded bridges, and iceoryx2 binding plumbing
//! are reused verbatim from `taktora-connector-can` (`REQ_0899`):
//! [`taktora_connector_can::CanHealthMonitor`],
//! [`taktora_connector_can::OutboundDrain`] /
//! [`taktora_connector_can::InboundPublish`], etc.

#![warn(missing_docs)]
// Allow J1939 domain identifiers (PGN, PDU1/PDU2, SA/DA, BAM, ETP,
// RTS/CTS) to appear in docstrings without backticks.
#![allow(clippy::doc_markdown)]

pub mod addr_claim;
pub mod connector;
pub mod decode;
pub mod dispatcher;
pub mod gateway;
pub mod mock;
pub mod options;
pub mod registry;
pub mod routing;
pub mod tp;
pub mod writer;

pub use addr_claim::{
    ADDRESS_CLAIM_PRIORITY, ADDRESS_CLAIMED_PGN, AcEvent, AcOutFrame, AddrClaimEngine,
    COMMANDED_ADDRESS_PGN, ClaimGate, ClaimState, DEFAULT_CLAIM_WAIT, NULL_ADDRESS,
    REQUEST_FOR_ADDRESS_CLAIMED, REQUEST_PGN,
};
pub use connector::{J1939Connector, J1939State};
pub use decode::{
    DecodedId, EXTENDED_ID_MASK, GLOBAL_ADDRESS, PDU2_FORMAT_THRESHOLD, decode_extended_id,
    encode_extended_id,
};
pub use dispatcher::{
    IterationOutcome, dispatch_one_iteration, dispatch_one_iteration_claim,
    dispatch_one_iteration_tp, dispatcher_loop,
};
pub use gateway::J1939Gateway;
pub use mock::MockJ1939Interface;
pub use options::{
    DEFAULT_ETP_INITIAL_SLICE_LEN, J1939ConnectorOptions, J1939ConnectorOptionsBuilder,
    J1939Interface,
};
pub use registry::{ChannelHandle, J1939Registry, RegisteredChannel};
pub use routing::{
    J1939Routing, PGN_MAX, Pgn, PgnError, SINGLE_FRAME_LEN, TP_MAX_LEN, TransportClass,
};
pub use tp::{
    BAM_CONTROL, CONN_ABORT_CONTROL, CTS_CONTROL, CTS_NO_LIMIT, DEFAULT_CTS_WINDOW,
    DEFAULT_MAX_ETP_BYTES, DEFAULT_MAX_TP_SESSIONS, EOMA_CONTROL, ETP_ABORT_CONTROL, ETP_CM_PGN,
    ETP_CTS_CONTROL, ETP_DPO_CONTROL, ETP_DT_PGN, ETP_EOMA_CONTROL, ETP_MIN_PAYLOAD,
    ETP_PROTOCOL_MAX, ETP_RTS_CONTROL, RTS_CONTROL, TP_CM_PGN, TP_DT_PGN, TP_MAX_PAYLOAD,
    TP_MIN_PAYLOAD, TpAbortReason, TpEngine, TpError, TpEvent, TpOutFrame, TpTimers,
};
pub use writer::J1939Writer;

// Re-export the large-payload slice handles the ETP connector surface
// returns (`ADR_0109` tier 2), so callers need not depend on
// `taktora-connector-transport-iox` directly.
pub use taktora_connector_transport_iox::{
    RecvSlice, SliceChannelReader, SliceChannelWriter, SliceSendOutcome,
};
