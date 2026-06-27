//! [`SliceChannelWriter`] / [`SliceChannelReader`] — a variable-length,
//! zero-copy channel pair over an iceoryx2 `[u8]` publish/subscribe
//! service. `BB_0097` / `FEAT_0097`.
//!
//! This is the **bulk path**, additive to (and independent of) the
//! fixed-`N` [`crate::ConnectorEnvelope`] POD envelope. Where the envelope
//! path inlines a compile-time `[u8; N]` buffer, this path loans a slice
//! sized to the message at send time via
//! [`iceoryx2::port::publisher::Publisher::loan_slice_uninit`]
//! (`REQ_0886`), so a single service carries messages of differing
//! lengths with one message per sample and no copy into a fixed buffer
//! (`REQ_0885`).
//!
//! The shared-memory data segment starts at a configurable
//! `initial_max_slice_len` and grows by powers of two
//! ([`AllocationStrategy`]) (`REQ_0887`), bounded by a
//! configurable `max_payload_bytes` ceiling: a send whose length exceeds
//! the ceiling is refused with a bounded-capacity
//! [`ConnectorError::PayloadOverflow`] **before** loaning, so the segment
//! never grows past the ceiling (`REQ_0888`).
//!
//! `sequence_number` (per-writer monotonic from zero) and `timestamp_ns`
//! (UNIX nanoseconds at loan time) carry the same semantics as the
//! [`crate::ConnectorEnvelope`] header, but ride an iceoryx2 **user-header**
//! ([`SliceUserHeader`]) rather than an inline POD struct (`REQ_0889`).
//!
//! [`AllocationStrategy`]: iceoryx2::prelude::AllocationStrategy

use core::sync::atomic::{AtomicU64, Ordering};

use iceoryx2::port::publisher::Publisher;
use iceoryx2::port::subscriber::Subscriber;
use iceoryx2::prelude::{ZeroCopySend, ipc};
use iceoryx2::sample::Sample;
use taktora_connector_core::ConnectorError;

use crate::now::now_unix_ns;

/// Per-sample metadata carried on the iceoryx2 user-header of every slice
/// sample (`REQ_0889`).
///
/// Mirrors the `sequence_number` / `timestamp_ns` semantics of
/// [`crate::ConnectorEnvelope`], but lives in the user-header rather than
/// inline in the payload.
///
/// `#[repr(C)]` + [`ZeroCopySend`] make this safe to publish through
/// iceoryx2's loan path.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, ZeroCopySend)]
pub struct SliceUserHeader {
    /// Per-(writer, channel) strictly monotonically increasing counter
    /// starting at zero. Same semantics as
    /// [`crate::ConnectorEnvelope::sequence_number`] (`REQ_0202`).
    pub sequence_number: u64,
    /// Nanoseconds since the UNIX epoch at the moment the sample was
    /// loaned for send. Same semantics as
    /// [`crate::ConnectorEnvelope::timestamp_ns`] (`REQ_0203`).
    pub timestamp_ns: u64,
}

/// Configuration for a [`SliceChannelWriter`]'s data segment (`REQ_0887`,
/// `REQ_0888`).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SliceChannelConfig {
    /// Initial maximum slice length the publisher's data segment is sized
    /// for. The segment grows from here by
    /// [`AllocationStrategy::PowerOfTwo`] as larger messages are loaned.
    ///
    /// [`AllocationStrategy::PowerOfTwo`]: iceoryx2::prelude::AllocationStrategy::PowerOfTwo
    pub initial_max_slice_len: usize,
    /// Hard ceiling on payload length. A [`SliceChannelWriter::send`]
    /// whose payload exceeds this is refused with
    /// [`ConnectorError::PayloadOverflow`] before loaning, so the segment
    /// never grows past this bound.
    pub max_payload_bytes: usize,
}

/// Outcome of a successful [`SliceChannelWriter::send`] call. Mirrors
/// [`crate::channel::SendOutcome`] for symmetry with the envelope path.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SliceSendOutcome {
    /// Sequence number stamped into the sample's user-header.
    pub sequence_number: u64,
    /// Timestamp stamped into the sample's user-header.
    pub timestamp_ns: u64,
    /// Number of payload bytes written into the loaned slice (equals the
    /// sent payload length).
    pub bytes_written: usize,
}

/// Variable-length publisher handle.
///
/// Owns an iceoryx2 [`Publisher`] over a `[u8]` slice payload with a
/// [`SliceUserHeader`] user-header, a per-handle monotonically increasing
/// sequence counter, and the `max_payload_bytes` ceiling.
pub struct SliceChannelWriter {
    inner: Publisher<ipc::Service, [u8], SliceUserHeader>,
    sequence: AtomicU64,
    max_payload_bytes: usize,
}

// SAFETY: same rationale as [`crate::ChannelWriter`] — iceoryx2
// publishers are conditionally `Send`, and the only per-call API used
// here (`loan_slice_uninit` / `send`) does not race with itself.
#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl Send for SliceChannelWriter {}

impl SliceChannelWriter {
    pub(crate) const fn new(
        inner: Publisher<ipc::Service, [u8], SliceUserHeader>,
        max_payload_bytes: usize,
    ) -> Self {
        Self {
            inner,
            sequence: AtomicU64::new(0),
            max_payload_bytes,
        }
    }

    /// The payload-length ceiling configured for this writer.
    #[must_use]
    pub const fn max_payload_bytes(&self) -> usize {
        self.max_payload_bytes
    }

    /// Publish `payload` as a single variable-length sample, sizing the
    /// loan to `payload.len()` (`REQ_0886`).
    ///
    /// The sequence number is claimed and the user-header stamped only
    /// after the loan succeeds; the timestamp is taken at loan time.
    ///
    /// # Errors
    ///
    /// Returns [`ConnectorError::PayloadOverflow`] — **without loaning**,
    /// so the data segment never grows past the ceiling (`REQ_0888`) —
    /// when `payload.len()` exceeds the configured `max_payload_bytes`.
    /// Returns [`ConnectorError::Stack`] wrapping any iceoryx2 loan / send
    /// error.
    pub fn send(&self, payload: &[u8]) -> Result<SliceSendOutcome, ConnectorError> {
        let len = payload.len();
        if len > self.max_payload_bytes {
            return Err(ConnectorError::PayloadOverflow {
                actual: len,
                max: self.max_payload_bytes,
            });
        }

        let sample = self
            .inner
            .loan_slice_uninit(len)
            .map_err(|e| ConnectorError::stack(SliceError(format!("loan: {e:?}"))))?;
        // `write_from_fn` initialises every element from the payload and
        // returns an initialised `SampleMut`.
        let mut sample = sample.write_from_fn(|i| payload[i]);

        let seq = self.sequence.fetch_add(1, Ordering::Relaxed);
        let ts = now_unix_ns();
        let header = sample.user_header_mut();
        header.sequence_number = seq;
        header.timestamp_ns = ts;

        sample
            .send()
            .map_err(|e| ConnectorError::stack(SliceError(format!("send: {e:?}"))))?;

        Ok(SliceSendOutcome {
            sequence_number: seq,
            timestamp_ns: ts,
            bytes_written: len,
        })
    }
}

/// Variable-length subscriber handle. Mirrors [`SliceChannelWriter`].
pub struct SliceChannelReader {
    inner: Subscriber<ipc::Service, [u8], SliceUserHeader>,
}

#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl Send for SliceChannelReader {}

impl SliceChannelReader {
    pub(crate) const fn new(inner: Subscriber<ipc::Service, [u8], SliceUserHeader>) -> Self {
        Self { inner }
    }

    /// Take the next sample, if any, as a zero-copy [`RecvSlice`].
    ///
    /// Returns `Ok(None)` when no sample is available. The returned
    /// [`RecvSlice`] borrows the iceoryx2 shared-memory sample directly —
    /// no `Vec` is materialised on this path.
    ///
    /// # Errors
    ///
    /// Returns [`ConnectorError::Stack`] wrapping any iceoryx2 receive
    /// error.
    pub fn try_recv(&self) -> Result<Option<RecvSlice>, ConnectorError> {
        let Some(sample) = self
            .inner
            .receive()
            .map_err(|e| ConnectorError::stack(SliceError(format!("receive: {e:?}"))))?
        else {
            return Ok(None);
        };
        Ok(Some(RecvSlice { sample }))
    }
}

/// A received variable-length sample. Owns the underlying iceoryx2
/// [`Sample`], keeping the shared-memory slice alive and zero-copy for
/// the lifetime of this handle.
///
/// This is the accessor shape the J1939 ETP reassembly path will consume:
/// [`Self::payload`] hands back a borrow of the SHM bytes (callers may
/// copy if they need ownership), and the per-sample metadata is read off
/// the user-header via [`Self::sequence_number`] / [`Self::timestamp_ns`].
pub struct RecvSlice {
    sample: Sample<ipc::Service, [u8], SliceUserHeader>,
}

impl RecvSlice {
    /// The sample's payload bytes. Length equals the sender's message
    /// length (not a fixed `N`).
    #[must_use]
    pub fn payload(&self) -> &[u8] {
        self.sample.payload()
    }

    /// Sequence number stamped by the sender (`REQ_0889`).
    #[must_use]
    pub fn sequence_number(&self) -> u64 {
        self.sample.user_header().sequence_number
    }

    /// Sender timestamp in UNIX nanoseconds (`REQ_0889`).
    #[must_use]
    pub fn timestamp_ns(&self) -> u64 {
        self.sample.user_header().timestamp_ns
    }
}

impl core::fmt::Debug for RecvSlice {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RecvSlice")
            .field("sequence_number", &self.sequence_number())
            .field("timestamp_ns", &self.timestamp_ns())
            .field("payload_len", &self.payload().len())
            .finish()
    }
}

/// Adapter that converts an iceoryx2 error string into a
/// [`std::error::Error`] for [`ConnectorError::stack`]. Internal — mirrors
/// the `IoxLoanError` / `RawError` newtype pattern.
#[derive(Debug)]
struct SliceError(String);

impl core::fmt::Display for SliceError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "iceoryx2 slice: {}", self.0)
    }
}

impl std::error::Error for SliceError {}
