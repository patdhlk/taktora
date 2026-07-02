//! Outbound-drain dispatcher loop and iceoryx2 adapters. `REQ_0252`,
//! `REQ_0253`, `REQ_0260`.
//!
//! The dispatcher runs on the gateway's tokio runtime once
//! [`crate::gateway::MqttGateway`] is started. Each tick it snapshots the
//! channel registry under the lock, then iterates lock-free: for every
//! [`ChannelBinding::Outbound`] it drains the iceoryx2 raw subscriber and
//! forwards the bytes to `session.publish(&routing, bytes)`. The full
//! [`crate::MqttRouting`] is passed through, so the session honours the
//! routing's QoS level (`REQ_0252`) and retained flag (`REQ_0253`).
//!
//! Inbound bindings are **not** iterated here — subscribe → fan-out is M2b.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use taktora_connector_core::ConnectorError;
use taktora_connector_transport_iox::{RawChannelReader, RawChannelWriter};
use tracing::warn;

use crate::bridge::{InboundBridge, InboundOutcome, OutboundBridge, OutboundError};
use crate::health::MqttHealthMonitor;
use crate::registry::{ChannelBinding, ChannelRegistry, InboundPublish, OutboundDrain};
use crate::routing::MqttRouting;
use crate::session::MqttSessionLike;

/// Default per-iteration outbound drain tick. Sets the upper bound on
/// outbound latency when the connector is otherwise idle.
pub const DEFAULT_DISPATCHER_TICK: Duration = Duration::from_millis(1);

/// Maximum scratch-buffer size the dispatcher allocates per drain
/// (heap-allocated once at loop entry). Channels with `N > MAX_DRAIN_SCRATCH`
/// fail the drain step; tune up if needed.
const MAX_DRAIN_SCRATCH: usize = 4096;

/// iceoryx2 outbound drain — wraps a [`RawChannelReader<N>`].
///
/// Implements [`OutboundDrain`] so the dispatcher can drain bytes from the
/// iceoryx2 raw subscriber as a trait object, erasing the const generic `N`
/// from the registry. The reader is `Mutex`-wrapped to give the drain
/// interior mutability behind a `Send + Sync` surface —
/// [`RawChannelReader`] is `Send` but not `Sync`, and the snapshot pattern
/// stores drains as `Arc<dyn OutboundDrain>`.
pub struct IoxOutboundDrain<const N: usize> {
    reader: Mutex<RawChannelReader<N>>,
}

impl<const N: usize> IoxOutboundDrain<N> {
    /// Wrap a `RawChannelReader` so the dispatcher can drain it as a trait
    /// object.
    #[must_use]
    pub const fn new(reader: RawChannelReader<N>) -> Self {
        Self {
            reader: Mutex::new(reader),
        }
    }
}

impl<const N: usize> OutboundDrain for IoxOutboundDrain<N> {
    fn drain_into(&self, dest: &mut [u8]) -> Result<Option<usize>, ConnectorError> {
        let sample_opt = {
            let reader = self.reader.lock().expect("outbound drain mutex poisoned");
            reader.try_recv_into(dest)?
        };
        Ok(sample_opt.map(|sample| sample.payload_len))
    }
}

/// iceoryx2 inbound publisher — wraps a [`RawChannelWriter<N>`].
///
/// Registered by `create_reader` so the inbound iox service exists, but the
/// subscribe → fan-out path that actually calls this is **M2b**; the M2a
/// dispatcher never drives it. `Mutex`-wrapped so concurrent session
/// callbacks can use the same publisher via `&self`.
pub struct IoxInboundPublish<const N: usize> {
    writer: Mutex<RawChannelWriter<N>>,
}

impl<const N: usize> IoxInboundPublish<N> {
    /// Wrap a `RawChannelWriter` so M2b session callbacks can republish
    /// bytes through it.
    #[must_use]
    pub const fn new(writer: RawChannelWriter<N>) -> Self {
        Self {
            writer: Mutex::new(writer),
        }
    }
}

impl<const N: usize> InboundPublish for IoxInboundPublish<N> {
    fn publish_bytes(&self, bytes: &[u8]) -> Result<(), ConnectorError> {
        let writer = self
            .writer
            .lock()
            .expect("inbound publisher mutex poisoned");
        writer.send_raw_bytes(bytes, [0u8; 32]).map(|_| ())
    }
}

/// Per-channel inbound publisher that gates the iceoryx2 send through an
/// [`InboundBridge`] for drop accounting (`REQ_0261`).
///
/// Wraps [`IoxInboundPublish`] with a bounded per-channel
/// [`InboundBridge<()>`] and the shared [`MqttHealthMonitor`]: when the
/// bridge is full the offending frame is dropped, the cumulative drop
/// count is incremented, and once it crosses the configured threshold the
/// monitor emits a single `Up → Degraded { reason: "dropped N inbound
/// frames" }` transition. Mirrors
/// `taktora_connector_zenoh::dispatcher::BridgedInboundPublish`.
pub struct BridgedInboundPublish<const N: usize> {
    iox: Option<IoxInboundPublish<N>>,
    bridge: InboundBridge<()>,
    health: Arc<MqttHealthMonitor>,
    threshold: u64,
}

impl<const N: usize> BridgedInboundPublish<N> {
    /// Construct a bridged publisher wired to an iceoryx2
    /// [`RawChannelWriter`] for the actual SHM transport.
    #[must_use]
    pub fn new(
        writer: RawChannelWriter<N>,
        capacity: usize,
        health: Arc<MqttHealthMonitor>,
        threshold: u64,
    ) -> Self {
        Self {
            iox: Some(IoxInboundPublish::new(writer)),
            bridge: InboundBridge::new(capacity),
            health,
            threshold,
        }
    }

    /// Construct a publisher with no iceoryx2 transport — used by
    /// `tests/saturation.rs`. Drop accounting + health transitions still
    /// run; bytes are silently swallowed instead of forwarded.
    #[must_use]
    pub fn without_transport(
        capacity: usize,
        health: Arc<MqttHealthMonitor>,
        threshold: u64,
    ) -> Self {
        Self {
            iox: None,
            bridge: InboundBridge::new(capacity),
            health,
            threshold,
        }
    }

    /// Borrow the per-channel bridge — used by tests to inspect the
    /// running drop count.
    #[must_use]
    pub const fn bridge(&self) -> &InboundBridge<()> {
        &self.bridge
    }
}

impl<const N: usize> InboundPublish for BridgedInboundPublish<N> {
    fn publish_bytes(&self, bytes: &[u8]) -> Result<(), ConnectorError> {
        match self.bridge.try_send(()) {
            InboundOutcome::Sent => {
                // The bridge is a bounded token counter; drain the token we
                // just enqueued so it tracks in-flight frames, then forward.
                let _ = self.bridge.try_recv();
                self.iox
                    .as_ref()
                    .map_or_else(|| Ok(()), |iox| iox.publish_bytes(bytes))
            }
            InboundOutcome::Dropped { count } => {
                // Drop the offending frame and account it. A single Degraded
                // transition fires once the threshold is crossed (`REQ_0261`).
                let _ = self.health.record_inbound_drop(count, self.threshold);
                Ok(())
            }
        }
    }
}

/// Outbound-bridge saturation gate (`REQ_0260`).
///
/// Wraps a bounded [`OutboundBridge`] with the shared [`MqttHealthMonitor`].
/// When the bridge is full, [`Self::try_send`] returns
/// [`ConnectorError::BackPressure`] and folds a single
/// `ConnectorHealth::Degraded` transition into the monitor. This is the
/// contract enforced on the outbound (plugin → gateway) path when a slow
/// broker lets the bridge fill; the bridge-level test in `tests/saturation.rs`
/// pins it (mirroring `taktora-connector-zenoh`'s saturation coverage).
pub struct BridgedOutbound<T> {
    bridge: OutboundBridge<T>,
    health: Arc<MqttHealthMonitor>,
}

impl<T> BridgedOutbound<T> {
    /// Construct a gate over a bounded bridge of `capacity`, wired to the
    /// shared health monitor.
    #[must_use]
    pub fn new(capacity: usize, health: Arc<MqttHealthMonitor>) -> Self {
        Self {
            bridge: OutboundBridge::new(capacity),
            health,
        }
    }

    /// Try to enqueue `msg` on the bridge. On saturation, records a single
    /// `Degraded` health transition and returns
    /// [`ConnectorError::BackPressure`] (`REQ_0260`).
    ///
    /// # Errors
    ///
    /// [`ConnectorError::BackPressure`] when the bridge is full or its
    /// gateway-side receiver has been dropped.
    pub fn try_send(&self, msg: T) -> Result<(), ConnectorError> {
        match self.bridge.try_send(msg) {
            Ok(()) => Ok(()),
            Err(OutboundError::BackPressure(_)) => {
                let _ = self.health.record_outbound_backpressure();
                Err(ConnectorError::BackPressure)
            }
            Err(OutboundError::Disconnected(_)) => Err(ConnectorError::BackPressure),
        }
    }

    /// Borrow the underlying bridge — used by tests to drain / inspect it.
    #[must_use]
    pub const fn bridge(&self) -> &OutboundBridge<T> {
        &self.bridge
    }
}

/// Drain every outbound channel once and forward each drained envelope to
/// `session.publish`, honouring the routing's QoS (`REQ_0252`) and retained
/// flag (`REQ_0253`). Returns the number of successful publishes.
///
/// Snapshots the registry under the lock, then iterates lock-free — the
/// async `session.publish` calls never hold the registry mutex across an
/// `.await`.
pub async fn dispatch_outbound_once<S>(
    registry: &Mutex<ChannelRegistry>,
    session: &Arc<S>,
    scratch: &mut [u8],
) -> usize
where
    S: MqttSessionLike,
{
    let entries: Vec<(MqttRouting, Arc<dyn OutboundDrain>, String)> = {
        let guard = registry.lock().expect("registry mutex poisoned");
        guard
            .iter()
            .filter_map(|e| match &e.binding {
                ChannelBinding::Outbound(drain) => Some((
                    e.routing.clone(),
                    Arc::clone(drain),
                    e.descriptor_name.to_string(),
                )),
                ChannelBinding::Inbound(_) => None,
            })
            .collect()
    };

    let mut published = 0usize;
    for (routing, drain, name) in entries {
        while let Ok(Some(n)) = drain.drain_into(scratch) {
            if let Err(e) = session.publish(&routing, &scratch[..n]).await {
                warn!(descriptor = %name, error = %e, "session.publish failed; dropping outbound chunk");
            } else {
                published += 1;
            }
        }
    }
    published
}

/// Outbound dispatcher loop. Runs on the gateway's tokio runtime until
/// `stop` is set, draining outbound channels each `tick` and forwarding to
/// `session.publish`.
///
/// # Errors
///
/// Currently infallible — per-publish failures are logged and skipped, not
/// propagated. Returns `Ok(())` when `stop` is observed. The `Result` is
/// kept for symmetry with the other connectors and forward-compatibility.
pub async fn dispatcher_loop<S>(
    registry: Arc<Mutex<ChannelRegistry>>,
    session: Arc<S>,
    stop: Arc<AtomicBool>,
    tick: Duration,
) -> Result<(), ConnectorError>
where
    S: MqttSessionLike,
{
    let mut scratch = vec![0u8; MAX_DRAIN_SCRATCH];
    while !stop.load(Ordering::Acquire) {
        let _ = dispatch_outbound_once(&registry, &session, &mut scratch).await;
        tokio::time::sleep(tick).await;
    }
    Ok(())
}
