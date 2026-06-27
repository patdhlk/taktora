//! [`J1939Writer`] — the address-claim-gated outbound writer. `BB_0102`,
//! `REQ_0898`.
//!
//! ## Why a wrapper
//!
//! `REQ_0898` requires the writer's `send` to return
//! [`ConnectorError::Down`] until the interface's address is `Claimed`
//! (consistent with the no-durable-buffering anti-goal `REQ_0292`). The
//! framework's [`ChannelWriter`] lives in
//! `taktora-connector-transport-iox` and is a **shared** transport type
//! used by every connector — it cannot carry connector-specific claim
//! state, and that crate must not be modified.
//!
//! The tension is resolved entirely inside the J1939 crate:
//! [`J1939Writer`] wraps the framework [`ChannelWriter`] plus an
//! <code>Arc<[ClaimGate]></code> shared with the per-interface
//! dispatcher's [`crate::addr_claim::AddrClaimEngine`]. Its `send` /
//! `send_with_correlation` return `Err(ConnectorError::Down { .. })`
//! while the gate is not `Claimed`, and delegate to the inner
//! [`ChannelWriter`] once it is. The `Connector` trait's `create_writer`
//! signature is untouched (it still returns the framework
//! [`ChannelWriter`]); the connector hands the application this gated
//! wrapper through the inherent
//! [`crate::J1939Connector::create_gated_writer`] method.
//!
//! TEST_0894 asserts on **this wrapper's** `send`: `Err(Down)` until
//! `Claimed`, then `Ok`.

use std::sync::Arc;

use taktora_connector_core::ConnectorError;
use taktora_connector_transport_iox::ChannelWriter;
use taktora_connector_transport_iox::channel::SendOutcome;
use taktora_connector_transport_iox::envelope::CorrelationId;

use crate::addr_claim::ClaimGate;

/// Address-claim-gated outbound writer over a framework [`ChannelWriter`].
///
/// Hand-back type of [`crate::J1939Connector::create_gated_writer`]. Until
/// the per-interface address is `Claimed`, `send` / `send_with_correlation`
/// return [`ConnectorError::Down`] (`REQ_0898`); afterwards they delegate
/// to the inner [`ChannelWriter`].
pub struct J1939Writer<T, C, const N: usize> {
    inner: ChannelWriter<T, C, N>,
    gate: Arc<ClaimGate>,
}

impl<T, C, const N: usize> J1939Writer<T, C, N>
where
    C: taktora_connector_core::PayloadCodec,
    T: serde::Serialize,
{
    /// Wrap `inner` with the per-interface address-claim `gate`.
    #[must_use]
    pub const fn new(inner: ChannelWriter<T, C, N>, gate: Arc<ClaimGate>) -> Self {
        Self { inner, gate }
    }

    /// Send `value` with a zeroed correlation id, gated on the address
    /// claim.
    ///
    /// # Errors
    ///
    /// [`ConnectorError::Down`] while the address is not `Claimed`
    /// (`REQ_0898`); otherwise any error from the inner
    /// [`ChannelWriter::send`].
    pub fn send(&self, value: &T) -> Result<SendOutcome, ConnectorError> {
        self.ensure_claimed()?;
        self.inner.send(value)
    }

    /// Send `value` with a caller-supplied correlation id, gated on the
    /// address claim.
    ///
    /// # Errors
    ///
    /// [`ConnectorError::Down`] while the address is not `Claimed`
    /// (`REQ_0898`); otherwise any error from the inner
    /// [`ChannelWriter::send_with_correlation`].
    pub fn send_with_correlation(
        &self,
        value: &T,
        correlation_id: CorrelationId,
    ) -> Result<SendOutcome, ConnectorError> {
        self.ensure_claimed()?;
        self.inner.send_with_correlation(value, correlation_id)
    }

    /// Borrow the shared claim gate (e.g. for tests to observe state).
    #[must_use]
    pub const fn gate(&self) -> &Arc<ClaimGate> {
        &self.gate
    }

    /// Borrow the wrapped framework writer.
    #[must_use]
    pub const fn inner(&self) -> &ChannelWriter<T, C, N> {
        &self.inner
    }

    fn ensure_claimed(&self) -> Result<(), ConnectorError> {
        if self.gate.is_claimed() {
            Ok(())
        } else {
            Err(ConnectorError::Down {
                reason: format!(
                    "j1939: outbound gated — address not claimed (state {:?})",
                    self.gate.state()
                ),
            })
        }
    }
}
