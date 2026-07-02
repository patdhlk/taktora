//! The async back-end seam (`MqttSessionLike`) abstracting real and mock
//! MQTT sessions, plus the connection-state and error types it carries.
//!
//! Modeled on `taktora_connector_zenoh::session::ZenohSessionLike`: the
//! I/O surface is async (stable `impl Future + Send`) because the real
//! `rumqttc` client is async, and the trait is only ever used as a generic
//! bound (never as `dyn MqttSessionLike`). `state()` stays synchronous — it
//! is a pure state read. M1 ships only the mock (`crate::mock`); the real
//! `rumqttc`-backed session lands in a later milestone.

use crate::routing::MqttRouting;
use crate::topic::MqttTopicFilter;

/// Callback invoked with the raw payload bytes of a delivered message.
pub type PayloadSink = Box<dyn Fn(&[u8]) + Send + Sync + 'static>;

/// The session's observable connection state. Maps to `ConnectorHealth`
/// variants in the gateway layer (`REQ_0980`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MqttConnectionState {
    /// The client is establishing the connection (pending / backing off).
    Connecting,
    /// A successful `CONNACK` was received; the session is operational.
    Connected,
    /// The session is disconnected; carries a human-readable reason.
    Disconnected {
        /// Reason for the disconnect (surfaced through `HealthEvent`).
        reason: String,
    },
}

/// Errors surfaced from [`MqttSessionLike`] operations.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// The session is not connected, so the operation cannot proceed.
    #[error("session not connected: {reason}")]
    NotConnected {
        /// Reason text from the connection-state snapshot.
        reason: String,
    },
    /// A publish failed at the session layer.
    #[error("publish failed: {reason}")]
    PublishFailed {
        /// Human-readable reason from the underlying back-end.
        reason: String,
    },
    /// A subscribe declaration failed at the session layer.
    #[error("subscribe failed: {reason}")]
    SubscribeFailed {
        /// Human-readable reason from the underlying back-end.
        reason: String,
    },
}

/// Opaque subscription handle. Dropping it tears down the subscription.
pub struct SubscriptionHandle(#[allow(dead_code)] pub(crate) Box<dyn std::any::Any + Send + Sync>);

impl std::fmt::Debug for SubscriptionHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SubscriptionHandle").finish_non_exhaustive()
    }
}

/// Abstraction over real and mock MQTT sessions.
///
/// The trait is used only as a generic bound (`S: MqttSessionLike`), never
/// as `dyn MqttSessionLike`, so the async methods can return
/// `impl Future + Send`.
pub trait MqttSessionLike: Send + Sync + 'static {
    /// Current observable connection state. Polled by the gateway to
    /// transition `ConnectorHealth`.
    fn state(&self) -> MqttConnectionState;

    /// Publish `payload` on the routing's topic at its QoS / retained
    /// setting.
    fn publish(
        &self,
        routing: &MqttRouting,
        payload: &[u8],
    ) -> impl std::future::Future<Output = Result<(), SessionError>> + Send;

    /// Subscribe to `filter`; `sink` is invoked for every message whose
    /// topic matches the filter. The returned handle unsubscribes on drop.
    fn subscribe(
        &self,
        filter: &MqttTopicFilter,
        sink: PayloadSink,
    ) -> impl std::future::Future<Output = Result<SubscriptionHandle, SessionError>> + Send;
}
