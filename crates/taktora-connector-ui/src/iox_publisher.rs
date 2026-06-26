//! [`IoxVmPublisher`]: the production [`VmPublisher`] backed by an iceoryx2
//! publish/subscribe service with **history depth 1** (latest-value, `REQ_0856`).
//!
//! A late-joining UI immediately receives the current value, and the pump can
//! cheaply ask how many subscribers are attached so it can skip publishing a
//! ViewModel nobody is watching (`REQ_0862`).
//!
//! # Why not the transport crate's raw writer
//!
//! [`taktora_connector_transport_iox::ServiceFactory::create_raw_writer_named`]
//! does not expose the service's subscriber count and creates the service
//! without `history_size(1)` (its default depth is shared with the EtherCAT
//! dispatcher, which must *not* retain history). The latest-value + skip
//! semantics this connector needs therefore build the service directly here,
//! while still publishing the same [`ConnectorEnvelope`] wire format the rest of
//! the framework uses, so a [`RawChannelReader`](taktora_connector_transport_iox::RawChannelReader)
//! or typed [`ChannelReader`](taktora_connector_transport_iox::ChannelReader)
//! reads it unchanged.

// iceoryx2 publishers and port factories are conditionally `Send`; the
// `VmPublisher` trait requires `Send` so the pump can own the publisher on its
// thread. The single `unsafe impl Send` below mirrors
// `RawChannelWriter`/`RawChannelReader` in the transport crate; the crate
// otherwise forbids unsafe via `#![deny(unsafe_code)]`.
#![allow(unsafe_code)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use iceoryx2::node::Node;
use iceoryx2::port::publisher::Publisher;
use iceoryx2::prelude::ipc;
use iceoryx2::service::port_factory::PortFactory as _;
use taktora_connector_core::ConnectorError;
use taktora_connector_transport_iox::ConnectorEnvelope;

use crate::pump::VmPublisher;

/// Per-subscriber queue depth, matched to the transport crate's
/// `ServiceFactory` (which uses 64) so a standard
/// [`RawChannelReader`](taktora_connector_transport_iox::RawChannelReader) or
/// typed [`ChannelReader`](taktora_connector_transport_iox::ChannelReader) can
/// `open_or_create` the same service without a buffer-size compatibility
/// mismatch. Must be `>= history_size`.
const SUBSCRIBER_MAX_BUFFER_SIZE: usize = 64;

/// Concrete publish/subscribe port factory type for our envelope.
type PsFactory<const N: usize> = iceoryx2::service::port_factory::publish_subscribe::PortFactory<
    ipc::Service,
    ConnectorEnvelope<N>,
    (),
>;

/// An iceoryx2-backed [`VmPublisher`] over [`ConnectorEnvelope<N>`].
///
/// Holds both the publisher (for sending) and the service port factory (for the
/// live subscriber count). `N` is the envelope payload capacity — a ViewModel's
/// `MAX_ENCODED_SIZE`.
pub struct IoxVmPublisher<const N: usize> {
    service: PsFactory<N>,
    publisher: Publisher<ipc::Service, ConnectorEnvelope<N>, ()>,
    sequence: AtomicU64,
}

// SAFETY: same rationale as `RawChannelWriter` in the transport crate — the
// iceoryx2 publisher and port factory are only ever used through `&self` methods
// (`send_copy`, `dynamic_config`) that do not race with themselves, and the pump
// owns exactly one `IoxVmPublisher` per service on a single thread.
#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl<const N: usize> Send for IoxVmPublisher<N> {}

impl<const N: usize> IoxVmPublisher<N> {
    /// Open (or create) the latest-value service named `name` on `node` and
    /// return a publisher for it.
    ///
    /// The service is configured with `history_size(1)` so a late-joining
    /// subscriber receives the current value on the publisher's next send.
    ///
    /// # Errors
    ///
    /// Returns [`ConnectorError::Configuration`] if `name` is not a valid
    /// iceoryx2 service name, or [`ConnectorError::Stack`] wrapping any
    /// iceoryx2 service / publisher creation error.
    pub fn create(node: &Node<ipc::Service>, name: &str) -> Result<Self, ConnectorError> {
        let service_name = name
            .try_into()
            .map_err(|e| ConnectorError::Configuration(format!("iceoryx2 name: {e:?}")))?;
        let service = node
            .service_builder(&service_name)
            .publish_subscribe::<ConnectorEnvelope<N>>()
            .history_size(1)
            .subscriber_max_buffer_size(SUBSCRIBER_MAX_BUFFER_SIZE)
            .open_or_create()
            .map_err(|e| ConnectorError::stack(IoxError(format!("open_or_create: {e:?}"))))?;
        let publisher = service
            .publisher_builder()
            .create()
            .map_err(|e| ConnectorError::stack(IoxError(format!("publisher: {e:?}"))))?;
        Ok(Self {
            service,
            publisher,
            sequence: AtomicU64::new(0),
        })
    }
}

impl<const N: usize> VmPublisher for IoxVmPublisher<N> {
    fn publish(&self, bytes: &[u8]) -> Result<(), ConnectorError> {
        if bytes.len() > N {
            return Err(ConnectorError::PayloadOverflow {
                actual: bytes.len(),
                max: N,
            });
        }
        let payload_len =
            u32::try_from(bytes.len()).map_err(|_| ConnectorError::PayloadOverflow {
                actual: bytes.len(),
                max: N,
            })?;

        // `ConnectorEnvelope` is `Copy`/`ZeroCopySend`; `send_copy` lets us stay
        // in safe code (no `loan_uninit` raw-pointer init).
        let mut envelope = ConnectorEnvelope::<N> {
            sequence_number: self.sequence.fetch_add(1, Ordering::Relaxed),
            timestamp_ns: now_unix_ns(),
            payload_len,
            ..Default::default()
        };
        envelope.payload[..bytes.len()].copy_from_slice(bytes);

        self.publisher
            .send_copy(envelope)
            .map_err(|e| ConnectorError::stack(IoxError(format!("send: {e:?}"))))?;
        Ok(())
    }

    fn subscriber_count(&self) -> usize {
        self.service.dynamic_config().number_of_subscribers()
    }
}

/// Nanoseconds since the UNIX epoch for the envelope timestamp (`REQ_0203`).
///
/// Wall-clock is correct here: the envelope timestamp is defined as time since
/// the UNIX epoch (unlike the process epoch in `system.rs`, which must avoid
/// wall-clock).
fn now_unix_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[derive(Debug)]
struct IoxError(String);

impl core::fmt::Display for IoxError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "iceoryx2 ui publisher: {}", self.0)
    }
}

impl std::error::Error for IoxError {}
