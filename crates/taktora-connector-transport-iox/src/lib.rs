//! iceoryx2-backed envelope transport for the taktora-connector framework.
//!
//! Implements [`BB_0002`](../../spec/architecture/connector.rst):
//!
//! * [`ConnectorEnvelope`] — POD wire format (`REQ_0200`, `REQ_0202`,
//!   `REQ_0203`, `REQ_0204`).
//! * [`ChannelWriter`] — zero-copy publisher using
//!   [`iceoryx2::port::publisher::Publisher::loan_uninit`] so the codec
//!   writes its bytes directly into shared memory (`REQ_0205`).
//! * [`ChannelReader`] — subscriber that decodes the envelope payload
//!   into `T` and surfaces codec errors as
//!   [`taktora_connector_core::ConnectorError::Codec`] (`REQ_0214`).
//! * [`ServiceFactory`] — opens / creates the iceoryx2 pub/sub service
//!   for a given [`taktora_connector_core::ChannelDescriptor`]
//!   (`REQ_0206`).
//! * [`SliceChannelWriter`] / [`SliceChannelReader`] — a variable-length,
//!   zero-copy channel pair over an iceoryx2 `[u8]` slice service
//!   (`BB_0097` / `FEAT_0097`). Additive to the fixed-`N`
//!   [`ConnectorEnvelope`] path: loans are sized to the message at send
//!   time, the data segment grows by `AllocationStrategy::PowerOfTwo`
//!   bounded by a configurable ceiling, and `sequence_number` /
//!   `timestamp_ns` ride an iceoryx2 user-header
//!   ([`SliceUserHeader`]) (`REQ_0885`–`REQ_0889`). This is the bulk path
//!   the J1939 ETP connector will later consume.
//!
//! The `Connector` trait itself (`REQ_0220`) lives in the host crate
//! `taktora-connector-host` because its method surface ties together
//! transport-iox handles with health subscription and lifecycle control
//! — concerns this crate intentionally does not own.

#![warn(missing_docs)]

pub mod channel;
pub mod envelope;
pub mod factory;
mod now;
pub mod raw;
pub mod slice;

pub use channel::{ChannelReader, ChannelWriter, RecvEnvelope};
pub use envelope::ConnectorEnvelope;
pub use factory::{ChannelSpec, ServiceFactory};
pub use raw::{RawChannelReader, RawChannelWriter, RawSample, RawSendOutcome};
pub use slice::{
    RecvSlice, SliceChannelConfig, SliceChannelReader, SliceChannelWriter, SliceSendOutcome,
    SliceUserHeader,
};
