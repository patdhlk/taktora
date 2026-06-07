//! [`EthercatHealthMonitor`] — thin wrapper around `HealthMonitor`.
//!
//! Broadcasts every emitted `HealthEvent` to every subscriber
//! (`REQ_0847`): each [`EthercatHealthMonitor::subscribe`] call opens
//! its own `crossbeam_channel`, so subscriptions are independent
//! streams — never competing consumers (#60).
//!
//! The wrapper centralises two concerns the bare `HealthMonitor` does
//! not own:
//!
//! 1. Thread-safe access. The bare monitor is `&mut`-only; the gateway
//!    side typically holds it behind a `Mutex` because both the tokio
//!    sidecar and the executor's `WaitSet` thread observe / mutate
//!    health.
//! 2. Fan-out. Every successful transition is rebroadcast to EVERY
//!    subscriber — one `crossbeam_channel::Sender` per subscription.

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
    /// One sender per live subscription (`REQ_0847`). Dead
    /// subscribers (dropped receivers) are pruned on each broadcast.
    subscribers: Mutex<Vec<Sender<HealthEvent>>>,
    degraded_due_to_drops: AtomicBool,
}

impl EthercatHealthMonitor {
    /// Construct a monitor in the initial `Connecting` state with no
    /// subscribers.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HealthMonitor::new()),
            subscribers: Mutex::new(Vec::new()),
            degraded_due_to_drops: AtomicBool::new(false),
        }
    }

    /// Broadcast `event` to every live subscriber, pruning
    /// subscriptions whose receiver has been dropped.
    fn broadcast(&self, event: &HealthEvent) {
        let mut subs = self
            .subscribers
            .lock()
            .expect("health subscriber list lock not poisoned");
        subs.retain(|tx| tx.send(event.clone()).is_ok());
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
    ///   per `ARCH_0012`. Broadcasting itself cannot fail: a
    ///   transition with zero (or only dropped) subscribers succeeds
    ///   and is observable via [`Self::current`].
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
        self.broadcast(&event);
        Ok(event)
    }

    /// Open an independent subscription (`REQ_0847`): the returned
    /// receiver observes every transition emitted AFTER this call.
    /// Multiple subscriptions never compete for events.
    ///
    /// Caveat: `Clone`-ing the returned receiver yields competing
    /// consumers on the SAME stream (`crossbeam_channel` is MPMC) —
    /// call `subscribe` again for an independent stream instead.
    ///
    /// # Panics
    ///
    /// Panics if the subscriber-list mutex was poisoned. See
    /// [`Self::current`] for the rationale.
    #[must_use]
    pub fn subscribe(&self) -> Receiver<HealthEvent> {
        let (tx, rx) = unbounded();
        self.subscribers
            .lock()
            .expect("health subscriber list lock not poisoned")
            .push(tx);
        rx
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
        self.broadcast(&event);
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
    /// Historical variant, **no longer emitted**: broadcasting is
    /// per-subscriber since `REQ_0847` and a transition with zero
    /// subscribers simply succeeds. Kept so removing it is not an
    /// API break.
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
