//! Shared topology and message definitions for the cross-process integrity
//! isolation example. Both the safety-critical and quality-managed processes
//! import this module to ensure they agree on channel names, message types,
//! and codec selection.

#![allow(clippy::doc_markdown)]

use serde::{Deserialize, Serialize};
use taktora_connector_core::{ChannelDescriptor, ConnectorError, PayloadCodec, Routing};

/// The shared iceoryx2 service name for the safety-to-quality channel.
/// Both processes must use the same name to communicate over the same
/// shared-memory service.
pub const CHANNEL_NAME: &str = "integrity_demo.sc_to_qm";

/// Maximum payload size in bytes for the channel. Small because our
/// messages are tiny POD structs.
pub const MAX_PAYLOAD_BYTES: usize = 512;

/// Number of cycles the safety process publishes and the quality process
/// expects to receive. Used by both binaries and the integration test to
/// coordinate a bounded run.
pub const CYCLE_COUNT: u64 = 100;

/// Message payload sent from the safety-critical process to the
/// quality-managed process each cycle.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CycleData {
    /// Monotonically increasing cycle number (0..CYCLE_COUNT).
    pub cycle: u64,
    /// Nanosecond timestamp when the message was created.
    pub timestamp_ns: u64,
}

/// Minimal `Routing` implementation for this example. The transport layer
/// does not inspect routing details; it only requires a `Routing` type
/// parameter for the `ChannelDescriptor`.
#[derive(Clone, Debug)]
pub struct DemoRouting;

impl Routing for DemoRouting {}

/// Build the channel descriptor for the safety-to-quality channel.
/// Returns a descriptor with the shared `CHANNEL_NAME` and
/// `MAX_PAYLOAD_BYTES`.
///
/// # Errors
///
/// Returns [`ConnectorError::Configuration`] if the channel name is invalid
/// (cannot happen with our static name, but the API is fallible).
pub fn channel_descriptor() -> Result<ChannelDescriptor<DemoRouting, MAX_PAYLOAD_BYTES>, ConnectorError> {
    ChannelDescriptor::new(CHANNEL_NAME, DemoRouting)
}

/// JSON codec for the example. Production code uses `JsonCodec` from
/// `taktora-connector-codec`; this stub keeps the example independent.
#[derive(Clone, Copy, Debug, Default)]
pub struct JsonCodec;

impl PayloadCodec for JsonCodec {
    fn format_name(&self) -> &'static str {
        "json"
    }

    fn encode<T>(&self, value: &T, buf: &mut [u8]) -> Result<usize, ConnectorError>
    where
        T: serde::Serialize,
    {
        let bytes = serde_json::to_vec(value).map_err(|e| ConnectorError::codec("json", e))?;
        if bytes.len() > buf.len() {
            return Err(ConnectorError::PayloadOverflow {
                actual: bytes.len(),
                max: buf.len(),
            });
        }
        buf[..bytes.len()].copy_from_slice(&bytes);
        Ok(bytes.len())
    }

    fn decode<T>(&self, buf: &[u8]) -> Result<T, ConnectorError>
    where
        T: serde::de::DeserializeOwned,
    {
        serde_json::from_slice(buf).map_err(|e| ConnectorError::codec("json", e))
    }
}

/// Read the current monotonic time in nanoseconds. Used to timestamp
/// outgoing messages.
#[must_use]
pub fn now_ns() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time is after the unix epoch")
        .as_nanos() as u64
}
