//! [`MqttHealthMonitor`] — thin wrapper around the framework
//! `taktora_connector_core::HealthMonitor`.
//!
//! Reuses the core state machine (`ARCH_0012`) and adds only the two
//! things the MQTT connector needs on top of it:
//!
//! 1. Subscriber fan-out over a `crossbeam_channel` so multiple observers
//!    get independent health streams (`REQ_0847`).
//! 2. The inbound-drop latch (`REQ_0261`): once cumulative inbound drops
//!    cross the configured threshold, emit a single
//!    `Up → Degraded { reason: "dropped N inbound frames" }` transition;
//!    the latch re-arms on the next `→ Up` transition.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use crossbeam_channel::{Receiver, Sender, unbounded};
use taktora_connector_core::{
    ConnectorError, ConnectorHealth, ConnectorHealthKind, HealthEvent, HealthMonitor,
    IllegalTransition,
};

/// Health monitor + broadcast channel pair for the MQTT connector.
#[derive(Debug)]
pub struct MqttHealthMonitor {
    inner: Mutex<HealthMonitor>,
    /// One sender per live subscription (`REQ_0847`). Dead subscribers
    /// (dropped receivers) are pruned on each broadcast.
    subscribers: Mutex<Vec<Sender<HealthEvent>>>,
    degraded_due_to_drops: AtomicBool,
    degraded_due_to_backpressure: AtomicBool,
}

impl MqttHealthMonitor {
    /// Construct a monitor in the initial `Connecting` state with no
    /// subscribers.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HealthMonitor::new()),
            subscribers: Mutex::new(Vec::new()),
            degraded_due_to_drops: AtomicBool::new(false),
            degraded_due_to_backpressure: AtomicBool::new(false),
        }
    }

    fn broadcast(&self, event: &HealthEvent) {
        let mut subs = self
            .subscribers
            .lock()
            .expect("mqtt health subscriber list lock not poisoned");
        subs.retain(|tx| tx.send(event.clone()).is_ok());
    }

    /// Snapshot the current health state.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex was poisoned by a prior panic.
    pub fn current(&self) -> ConnectorHealth {
        self.inner
            .lock()
            .expect("mqtt health monitor lock not poisoned")
            .current()
            .clone()
    }

    /// Open an independent subscription (`REQ_0847`): the returned receiver
    /// observes every transition emitted AFTER this call.
    ///
    /// # Panics
    ///
    /// Panics if the subscriber-list mutex was poisoned.
    #[must_use]
    pub fn subscribe(&self) -> Receiver<HealthEvent> {
        let (tx, rx) = unbounded();
        self.subscribers
            .lock()
            .expect("mqtt health subscriber list lock not poisoned")
            .push(tx);
        rx
    }

    /// Try to transition to `target`, broadcasting the emitted event to
    /// every subscriber on success. Recovery to `Up` re-arms the
    /// inbound-drop latch.
    ///
    /// # Errors
    ///
    /// Returns [`MqttHealthError::Illegal`] when the from→to pair is not
    /// allowed per `ARCH_0012`.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex was poisoned by a prior panic.
    pub fn transition_to(&self, target: ConnectorHealth) -> Result<HealthEvent, MqttHealthError> {
        let event = {
            let mut guard = self
                .inner
                .lock()
                .expect("mqtt health monitor lock not poisoned");
            guard
                .try_transition_to(target)
                .map_err(MqttHealthError::Illegal)?
        };
        if event.to.kind() == ConnectorHealthKind::Up {
            self.degraded_due_to_drops.store(false, Ordering::Release);
            self.degraded_due_to_backpressure
                .store(false, Ordering::Release);
        }
        self.broadcast(&event);
        Ok(event)
    }

    /// Record a cumulative inbound-drop count from an
    /// [`crate::InboundOutcome::Dropped`] return. When `count` crosses
    /// `threshold`, the latch is unset, and the monitor is currently `Up`,
    /// emit a single `Up → Degraded { reason: "dropped N inbound frames" }`
    /// transition and set the latch (`REQ_0261`).
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex was poisoned by a prior panic.
    pub fn record_inbound_drop(&self, count: u64, threshold: u64) -> Option<HealthEvent> {
        if count < threshold || self.degraded_due_to_drops.load(Ordering::Acquire) {
            return None;
        }
        let event = {
            let mut guard = self
                .inner
                .lock()
                .expect("mqtt health monitor lock not poisoned");
            if guard.current().kind() != ConnectorHealthKind::Up {
                // Not `Up` — already Degraded/Connecting/Down for another
                // reason. Latch so we stop re-checking on every drop, and
                // skip emitting (the spec's "already Degraded" clause).
                self.degraded_due_to_drops.store(true, Ordering::Release);
                return None;
            }
            let target = ConnectorHealth::Degraded {
                reason: format!("dropped {count} inbound frames"),
            };
            guard.try_transition_to(target).ok()?
        };
        self.degraded_due_to_drops.store(true, Ordering::Release);
        self.broadcast(&event);
        Some(event)
    }

    /// Record an outbound-bridge saturation event (`REQ_0260`). When the
    /// latch is unset and the monitor is currently `Up` or `Connecting`,
    /// emit a single `→ Degraded { reason: "outbound backpressure" }`
    /// transition and set the latch. The latch re-arms on the next `→ Up`
    /// transition, so a burst of back-pressure yields at most one
    /// `Degraded` event until the connector recovers.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex was poisoned by a prior panic.
    pub fn record_outbound_backpressure(&self) -> Option<HealthEvent> {
        if self.degraded_due_to_backpressure.load(Ordering::Acquire) {
            return None;
        }
        let event = {
            let mut guard = self
                .inner
                .lock()
                .expect("mqtt health monitor lock not poisoned");
            let kind = guard.current().kind();
            let recoverable = matches!(
                kind,
                ConnectorHealthKind::Up | ConnectorHealthKind::Connecting
            );
            if !recoverable {
                // Already Degraded/Down for another reason — latch so we
                // stop re-checking, and skip emitting.
                self.degraded_due_to_backpressure
                    .store(true, Ordering::Release);
                return None;
            }
            let target = ConnectorHealth::Degraded {
                reason: "outbound backpressure".to_string(),
            };
            guard.try_transition_to(target).ok()?
        };
        self.degraded_due_to_backpressure
            .store(true, Ordering::Release);
        self.broadcast(&event);
        Some(event)
    }

    /// Test/inspection helper: snapshot the drops-latch state.
    #[must_use]
    pub fn degraded_due_to_drops(&self) -> bool {
        self.degraded_due_to_drops.load(Ordering::Acquire)
    }

    /// Test/inspection helper: snapshot the backpressure-latch state.
    #[must_use]
    pub fn degraded_due_to_backpressure(&self) -> bool {
        self.degraded_due_to_backpressure.load(Ordering::Acquire)
    }
}

impl Default for MqttHealthMonitor {
    fn default() -> Self {
        Self::new()
    }
}

/// Failure modes of [`MqttHealthMonitor::transition_to`].
#[derive(Debug, thiserror::Error)]
pub enum MqttHealthError {
    /// Requested from→to transition is not allowed by `ARCH_0012`.
    #[error(transparent)]
    Illegal(#[from] IllegalTransition),
}

impl From<MqttHealthError> for ConnectorError {
    fn from(err: MqttHealthError) -> Self {
        match err {
            MqttHealthError::Illegal(e) => Self::stack(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_monitor_is_connecting() {
        let m = MqttHealthMonitor::new();
        assert_eq!(m.current().kind(), ConnectorHealthKind::Connecting);
    }

    #[test]
    fn subscriber_receives_transitions() {
        let m = MqttHealthMonitor::new();
        let sub = m.subscribe();
        let evt = m.transition_to(ConnectorHealth::Up).unwrap();
        assert_eq!(evt.to.kind(), ConnectorHealthKind::Up);
        let received = sub.try_recv().unwrap();
        assert_eq!(received.to.kind(), ConnectorHealthKind::Up);
    }

    /// `REQ_0261`: crossing the drop threshold while `Up` emits a single
    /// `Up → Degraded`; further drops are latched out; recovery to `Up`
    /// re-arms the latch.
    #[test]
    fn record_inbound_drop_emits_degraded_once() {
        let m = MqttHealthMonitor::new();
        let _ = m.transition_to(ConnectorHealth::Up).unwrap();
        let sub = m.subscribe();

        let evt = m
            .record_inbound_drop(1, 1)
            .expect("first cross-threshold emits Degraded");
        assert_eq!(evt.from.kind(), ConnectorHealthKind::Up);
        assert_eq!(evt.to.kind(), ConnectorHealthKind::Degraded);
        match &evt.to {
            ConnectorHealth::Degraded { reason } => {
                assert!(reason.contains("dropped") && reason.contains('1'));
            }
            other => panic!("expected Degraded, got {other:?}"),
        }
        assert!(m.degraded_due_to_drops());
        assert_eq!(
            sub.try_recv().unwrap().to.kind(),
            ConnectorHealthKind::Degraded
        );

        // Latched: subsequent drops emit nothing.
        assert!(m.record_inbound_drop(2, 1).is_none());
        assert!(m.record_inbound_drop(3, 1).is_none());

        // Recovery to Up re-arms the latch.
        let _ = m.transition_to(ConnectorHealth::Up).unwrap();
        assert!(!m.degraded_due_to_drops());
    }
}
