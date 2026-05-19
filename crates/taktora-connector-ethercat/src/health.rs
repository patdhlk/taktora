//! [`EthercatHealthMonitor`] — thin wrapper around `HealthMonitor`.
//!
//! Broadcasts every emitted `HealthEvent` over a `crossbeam_channel`
//! so observers (e.g. `EthercatConnector::subscribe_health`) can
//! listen.
//!
//! The wrapper centralises two concerns the bare `HealthMonitor` does
//! not own:
//!
//! 1. Thread-safe access. The bare monitor is `&mut`-only; the gateway
//!    side typically holds it behind a `Mutex` because both the tokio
//!    sidecar and the executor's `WaitSet` thread observe / mutate
//!    health.
//! 2. Fan-out. Every successful transition is rebroadcast to one or
//!    more subscribers via a `crossbeam_channel::Sender`.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use crossbeam_channel::{Receiver, Sender, unbounded};
use taktora_connector_core::{
    ConnectorError, ConnectorHealth, ConnectorHealthKind, HealthEvent, HealthMonitor,
    IllegalTransition,
};

/// Health monitor + broadcast channel pair.
///
/// Also carries the inbound-drop latch (`REQ_0324`): the gateway emits
/// a single `Up → Degraded { reason: "dropped N inbound frames" }`
/// transition once cumulative drops cross the configured threshold;
/// the latch re-arms on the next stack-driven `→ Up` transition.
#[derive(Debug)]
pub struct EthercatHealthMonitor {
    inner: Mutex<HealthMonitor>,
    tx: Sender<HealthEvent>,
    rx: Receiver<HealthEvent>,
    degraded_due_to_drops: AtomicBool,
}

impl EthercatHealthMonitor {
    /// Construct a monitor in the initial `Connecting` state with an
    /// unbounded broadcast channel.
    #[must_use]
    pub fn new() -> Self {
        let (tx, rx) = unbounded();
        Self {
            inner: Mutex::new(HealthMonitor::new()),
            tx,
            rx,
            degraded_due_to_drops: AtomicBool::new(false),
        }
    }

    /// Snapshot the current state.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex has been poisoned by a previous
    /// panicked call. The monitor's methods are short and panic-free
    /// in normal operation, so a poisoned lock indicates a serious
    /// bug elsewhere — fail fast rather than mask.
    pub fn current(&self) -> ConnectorHealth {
        self.inner
            .lock()
            .expect("health monitor lock not poisoned")
            .current()
            .clone()
    }

    /// Try to transition to `target`. On success the emitted
    /// `HealthEvent` is broadcast to every subscriber.
    ///
    /// # Errors
    ///
    /// * [`IllegalTransition`] when the from→to pair is not allowed
    ///   per `ARCH_0012`.
    /// * [`ConnectorError::Stack`] if the broadcast channel has lost
    ///   all subscribers (impossible by construction — `self` holds
    ///   the receive end).
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned by a previous
    /// panicked transition. See [`Self::current`] for the rationale.
    pub fn transition_to(
        &self,
        target: ConnectorHealth,
    ) -> Result<HealthEvent, EthercatHealthError> {
        let event = {
            let mut guard = self.inner.lock().expect("health monitor lock not poisoned");
            guard
                .try_transition_to(target)
                .map_err(EthercatHealthError::Illegal)?
        };
        // Recovery to Up re-arms the drops-latch (`REQ_0324`).
        if event.to.kind() == ConnectorHealthKind::Up {
            self.degraded_due_to_drops.store(false, Ordering::Release);
        }
        self.tx
            .send(event.clone())
            .map_err(|_| EthercatHealthError::BroadcastClosed)?;
        Ok(event)
    }

    /// Subscriber-side receiver. Each `Clone` of the returned handle
    /// observes the same stream — `crossbeam_channel` is MPMC.
    #[must_use]
    pub fn subscribe(&self) -> Receiver<HealthEvent> {
        self.rx.clone()
    }

    /// Record a cumulative inbound-drop count from one channel's
    /// [`crate::InboundOutcome::Dropped`] return. Emits a single
    /// `Up → Degraded { reason: "dropped N inbound frames" }` transition
    /// when `count` crosses `threshold` AND the drops-latch is unset
    /// AND the current state is `Up` (`REQ_0324`).
    ///
    /// Returns the emitted `HealthEvent` when a transition actually
    /// fired.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex has been poisoned by a previous
    /// panicked transition. See [`Self::current`] for the rationale.
    pub fn record_inbound_drop(&self, count: u64, threshold: u64) -> Option<HealthEvent> {
        if count < threshold || self.degraded_due_to_drops.load(Ordering::Acquire) {
            return None;
        }
        let event = {
            let mut guard = self.inner.lock().expect("health monitor lock not poisoned");
            if guard.current().kind() != ConnectorHealthKind::Up {
                self.degraded_due_to_drops.store(true, Ordering::Release);
                return None;
            }
            let target = ConnectorHealth::Degraded {
                reason: format!("dropped {count} inbound frames"),
            };
            guard.try_transition_to(target).ok()?
        };
        self.degraded_due_to_drops.store(true, Ordering::Release);
        let _ = self.tx.send(event.clone());
        Some(event)
    }

    /// Test helper: snapshot the drops-latch state.
    #[must_use]
    pub fn degraded_due_to_drops(&self) -> bool {
        self.degraded_due_to_drops.load(Ordering::Acquire)
    }
}

impl Default for EthercatHealthMonitor {
    fn default() -> Self {
        Self::new()
    }
}

/// Failure modes of [`EthercatHealthMonitor::transition_to`].
#[derive(Debug, thiserror::Error)]
pub enum EthercatHealthError {
    /// Requested from→to transition not allowed by `ARCH_0012`.
    #[error(transparent)]
    Illegal(#[from] IllegalTransition),
    /// Broadcast channel has no receivers — should not happen
    /// because the monitor holds an internal receiver clone.
    #[error("health broadcast channel closed")]
    BroadcastClosed,
}

impl From<EthercatHealthError> for ConnectorError {
    fn from(err: EthercatHealthError) -> Self {
        match err {
            EthercatHealthError::Illegal(e) => Self::stack(e),
            EthercatHealthError::BroadcastClosed => Self::Down {
                reason: "health broadcast closed".to_string(),
            },
        }
    }
}
