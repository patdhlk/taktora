//! Executor heartbeat to `HealthEvent` bridge.
//!
//! Bridges the executor's liveness heartbeat
//! ([`taktora_executor::Observer::on_heartbeat`]) onto a `HealthEvent`
//! publish path for external watchdog integration. Supports `TSR_0010` /
//! `AOU_0003`.

use std::time::Instant;
use taktora_connector_core::{ConnectorHealth, HealthEvent};
use taktora_executor::{HeartbeatTick, Observer};

/// Bridges executor heartbeat ticks to a `HealthEvent` channel.
///
/// Implements [`Observer`] and forwards every [`HeartbeatTick`] as a
/// `HealthEvent` (from `ConnectorHealth::Up` to `ConnectorHealth::Up`) on
/// the given sender. An integrator wires this as the executor's observer
/// (or composes it with other observers) to expose liveness on the
/// connector's health-event path.
///
/// # Example
///
/// ```no_run
/// use crossbeam_channel::unbounded;
/// use std::sync::Arc;
/// use std::time::Duration;
/// use taktora_connector_host::HeartbeatHealthBridge;
/// use taktora_executor::{Executor, Observer};
///
/// let (health_tx, health_rx) = unbounded();
/// let bridge = Arc::new(HeartbeatHealthBridge::new(health_tx));
///
/// let mut exec = Executor::builder()
///     .heartbeat(Duration::from_millis(50))
///     .observer(bridge as Arc<dyn Observer>)
///     .build()
///     .unwrap();
///
/// // Start executor; health_rx receives a HealthEvent per tick.
/// ```
///
/// # TSR coverage
///
/// Supports `TSR_0010` / `AOU_0003`: bridges the executor's bounded-period
/// liveness signal onto a `HealthEvent` path so an external monitor can
/// force safe state on omission.
pub struct HeartbeatHealthBridge {
    /// Channel sender for forwarding ticks as `HealthEvent`.
    health_tx: crossbeam_channel::Sender<HealthEvent>,
}

impl HeartbeatHealthBridge {
    /// Create a new bridge that forwards heartbeat ticks to `health_tx`.
    ///
    /// Each tick is translated to a `HealthEvent` with `from: Up`,
    /// `to: Up`, and `at: Instant::now()`. The receiver can use these
    /// events as a liveness signal: if no event arrives within a configured
    /// timeout (typically FTTI), the monitor forces a safe state.
    #[must_use]
    pub const fn new(health_tx: crossbeam_channel::Sender<HealthEvent>) -> Self {
        Self { health_tx }
    }
}

impl Observer for HeartbeatHealthBridge {
    fn on_heartbeat(&self, _tick: &HeartbeatTick) {
        // Translate the tick into a HealthEvent: from Up to Up represents
        // a liveness heartbeat (no state change, just a periodic signal).
        let event = HealthEvent {
            from: ConnectorHealth::Up,
            to: ConnectorHealth::Up,
            at: Instant::now(),
        };

        // Send is non-blocking. If the receiver is gone (disconnected),
        // the send fails silently — the executor continues; the bridge
        // degrades gracefully (no watchdog, but runtime still operates).
        let _ = self.health_tx.try_send(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::unbounded;

    #[test]
    fn forwards_heartbeat_as_health_event() {
        let (tx, rx) = unbounded();
        let bridge = HeartbeatHealthBridge::new(tx);

        let tick = HeartbeatTick {
            seq: 1,
            at_nanos: 1_000_000,
        };

        bridge.on_heartbeat(&tick);

        let event = rx.try_recv().expect("expected a HealthEvent");
        assert!(matches!(event.from, ConnectorHealth::Up));
        assert!(matches!(event.to, ConnectorHealth::Up));
        // `at` is Instant::now(), so we can't assert exact value, but it
        // should be recent.
    }

    #[test]
    fn multiple_ticks_produce_multiple_events() {
        let (tx, rx) = unbounded();
        let bridge = HeartbeatHealthBridge::new(tx);

        for seq in 1..=3 {
            let tick = HeartbeatTick {
                seq,
                at_nanos: seq * 1_000_000,
            };
            bridge.on_heartbeat(&tick);
        }

        let events: Vec<_> = rx.try_iter().collect();
        assert_eq!(events.len(), 3);
        for event in events {
            assert!(matches!(event.from, ConnectorHealth::Up));
            assert!(matches!(event.to, ConnectorHealth::Up));
        }
    }

    #[test]
    fn graceful_when_receiver_disconnected() {
        let (tx, rx) = unbounded();
        let bridge = HeartbeatHealthBridge::new(tx);

        // Drop the receiver.
        drop(rx);

        // Forwarding should not panic.
        let tick = HeartbeatTick {
            seq: 1,
            at_nanos: 1_000_000,
        };
        bridge.on_heartbeat(&tick);
    }
}
