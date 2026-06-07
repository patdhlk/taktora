//! [`ZenohHealthMonitor`] — thin wrapper around `HealthMonitor`.
//!
//! Broadcasts every emitted `HealthEvent` over a `crossbeam_channel`
//! so observers (e.g. `ZenohGateway::subscribe_health` in Z2) can
//! listen.
//!
//! The wrapper centralises two concerns the bare `HealthMonitor` does
//! not own:
//!
//! 1. Thread-safe access. The bare monitor is `&mut`-only; the gateway
//!    side typically holds it behind a `Mutex` because both async tasks
//!    and synchronous observer threads may observe / mutate health.
//! 2. Fan-out. Every successful transition is rebroadcast to one or
//!    more subscribers via a `crossbeam_channel::Sender`.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use crossbeam_channel::{Receiver, Sender, unbounded};
use taktora_connector_core::{
    ConnectorError, ConnectorHealth, ConnectorHealthKind, HealthEvent, HealthMonitor,
    IllegalTransition,
};

use crate::session::SessionState;

/// Health monitor + broadcast channel pair.
///
/// Also carries the inbound-drop latch (`REQ_0406`, `REQ_0428`): the
/// gateway emits a single `Up → Degraded { reason: "dropped N inbound frames" }`
/// transition once cumulative drops cross the configured threshold;
/// the latch re-arms on the next stack-driven `→ Up` transition.
#[derive(Debug)]
pub struct ZenohHealthMonitor {
    inner: Mutex<HealthMonitor>,
    /// One sender per live subscription (`REQ_0847`). Dead
    /// subscribers (dropped receivers) are pruned on each broadcast.
    subscribers: Mutex<Vec<Sender<HealthEvent>>>,
    degraded_due_to_drops: AtomicBool,
}

impl ZenohHealthMonitor {
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
            .expect("zenoh health subscriber list lock not poisoned");
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
    /// * [`ZenohHealthError::Illegal`] when the from→to pair is not
    ///   allowed per `ARCH_0012`. Broadcasting itself cannot fail: a
    ///   transition with zero (or only dropped) subscribers succeeds
    ///   and is observable via [`Self::current`].
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned by a previous
    /// panicked transition. See [`Self::current`] for the rationale.
    pub fn transition_to(&self, target: ConnectorHealth) -> Result<HealthEvent, ZenohHealthError> {
        let event = {
            let mut guard = self.inner.lock().expect("health monitor lock not poisoned");
            guard
                .try_transition_to(target)
                .map_err(ZenohHealthError::Illegal)?
        };
        // Recovery to Up re-arms the drops-latch (`REQ_0406`, `REQ_0428`).
        if event.to.kind() == ConnectorHealthKind::Up {
            self.degraded_due_to_drops.store(false, Ordering::Release);
        }
        self.broadcast(&event);
        Ok(event)
    }

    /// Record a cumulative inbound-drop count from one channel's
    /// [`crate::InboundOutcome::Dropped`] return. Emits a single
    /// `Up → Degraded { reason: "dropped N inbound frames" }` transition
    /// when `count` crosses `threshold` AND the drops-latch is unset
    /// AND the current state is `Up` (`REQ_0406`, `REQ_0428`).
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
                // Already Degraded / Connecting / Down — latch so we
                // do not re-check on every dropped frame, and skip
                // emitting per the spec's "skip emitting if already
                // Degraded for another reason" clause.
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
            .expect("zenoh health subscriber list lock not poisoned")
            .push(tx);
        rx
    }

    /// Apply a single observation of the session — current state plus
    /// the currently linked peer count — to the monitor's state
    /// machine. Used by the Z5b health watcher task: every poll tick
    /// produces one observation, and at most one [`HealthEvent`] hits
    /// the broadcast channel.
    ///
    /// Mapping (per `ARCH_0012`'s reachable edges, refined by
    /// `REQ_0442` for the `min_peers` floor):
    ///
    /// * `SessionState::Connecting` → `ConnectorHealth::Connecting`.
    /// * `SessionState::Alive` + `peer_count >= floor`
    ///   (or `min_peers.is_none()`) → `ConnectorHealth::Up`.
    /// * `SessionState::Alive` + `peer_count < floor` →
    ///   `ConnectorHealth::Degraded { reason }`.
    /// * `SessionState::Closed { reason }` →
    ///   `ConnectorHealth::Down { reason }`.
    ///
    /// Illegal transitions per the health state machine (e.g. observing
    /// `Alive` while already `Up`) are dropped silently — the watcher
    /// should not panic on a benign no-op. The one wrinkle: the monitor
    /// starts in `Connecting`, but the watcher's very first observation
    /// can already imply `Degraded` (an `Alive` session whose peer count
    /// is below the floor). `ARCH_0012` does not allow a direct
    /// `Connecting -> Degraded` edge, so we silently bridge through
    /// `Up` (no broadcast for the bridge step) and broadcast only the
    /// final `Up -> Degraded` event.
    pub(crate) fn apply_observation(
        &self,
        state: &SessionState,
        peer_count: usize,
        min_peers: Option<usize>,
    ) {
        let target = match state {
            SessionState::Connecting => ConnectorHealth::Connecting {
                since: Instant::now(),
            },
            SessionState::Alive => match min_peers {
                Some(floor) if peer_count < floor => ConnectorHealth::Degraded {
                    reason: format!("linked peers {peer_count} < min_peers {floor}"),
                },
                _ => ConnectorHealth::Up,
            },
            SessionState::Closed { reason } => ConnectorHealth::Down {
                reason: reason.clone(),
                since: Instant::now(),
            },
        };

        // Bridge `Connecting -> Degraded` (illegal direct edge per
        // `ARCH_0012`) by silently advancing the monitor through `Up`
        // without broadcasting the intermediate event. The caller sees
        // exactly one `HealthEvent`: the final `Up -> Degraded`.
        let event = {
            let mut guard = self.inner.lock().expect("health monitor lock not poisoned");
            if guard.current().kind() == ConnectorHealthKind::Connecting
                && target.kind() == ConnectorHealthKind::Degraded
            {
                // Best-effort bridge; if `Connecting -> Up` is somehow
                // illegal in a future revision of the matrix, we still
                // fall through to the final `try_transition_to` which
                // will return an `IllegalTransition` we then drop.
                let _ = guard.try_transition_to(ConnectorHealth::Up);
            }
            guard.try_transition_to(target).ok()
        };
        if let Some(ev) = event {
            // Recovery to Up via the session-state observation
            // re-arms the drops-latch (`REQ_0406`, `REQ_0428`).
            if ev.to.kind() == ConnectorHealthKind::Up {
                self.degraded_due_to_drops.store(false, Ordering::Release);
            }
            self.broadcast(&ev);
        }
    }
}

impl Default for ZenohHealthMonitor {
    fn default() -> Self {
        Self::new()
    }
}

/// Failure modes of [`ZenohHealthMonitor::transition_to`].
#[derive(Debug, thiserror::Error)]
pub enum ZenohHealthError {
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

impl From<ZenohHealthError> for ConnectorError {
    fn from(err: ZenohHealthError) -> Self {
        match err {
            ZenohHealthError::Illegal(e) => Self::stack(e),
            ZenohHealthError::BroadcastClosed => Self::Down {
                reason: "health broadcast closed".to_string(),
            },
        }
    }
}
