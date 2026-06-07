//! [`CanHealthMonitor`] — wraps `taktora_connector_core::HealthMonitor`
//! with per-interface sub-state and worst-of aggregation.
//! `REQ_0630`, `REQ_0635`.
//!
//! Aggregation rule (`REQ_0630`): the externally-visible
//! [`ConnectorHealth`] is the worst of every iface's sub-state.
//!
//! * Every iface `Up` ⇒ connector `Up`.
//! * Some iface `Degraded` (others `Up`) ⇒ connector `Degraded`.
//! * Some iface `Down` but ≥ 1 `Up` ⇒ connector `Degraded` (iface
//!   down is degraded service overall, not total loss).
//! * All ifaces `Down` ⇒ connector `Down`.
//! * Mixed `Connecting` / `Up` during bring-up ⇒ connector
//!   `Connecting` until at least one iface is `Up`.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use crossbeam_channel::{Receiver, Sender, unbounded};
use taktora_connector_core::{
    ConnectorError, ConnectorHealth, ConnectorHealthKind, HealthEvent, HealthMonitor,
    IllegalTransition,
};

use crate::routing::CanIface;

/// Per-interface health discriminator. Maps onto the connector's
/// externally-visible [`ConnectorHealth`] via the worst-of aggregator
/// inside [`CanHealthMonitor::set_iface`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IfaceHealthKind {
    /// Socket open, no traffic seen yet.
    Connecting,
    /// Operational.
    Up,
    /// Error-warning / error-passive (`REQ_0632`).
    Degraded,
    /// Bus-off; awaiting reconnect (`REQ_0633`).
    Down,
}

#[derive(Debug)]
struct IfaceState {
    kind: IfaceHealthKind,
    last_error_at: Option<Instant>,
}

/// Health monitor + broadcast channel pair.
///
/// Owns the framework-level `HealthMonitor` plus a per-iface map.
/// Each per-iface update may emit a `HealthEvent` on the aggregated
/// stream — only when the worst-of aggregation transitions to a new
/// state per `ARCH_0012`.
///
/// Also carries the inbound-drop latch (`REQ_0608`): once cumulative
/// drops cross the configured threshold the monitor emits a single
/// `Up → Degraded { reason: "dropped N inbound frames" }` transition;
/// the latch re-arms on the next stack-driven `→ Up` transition.
#[derive(Debug)]
pub struct CanHealthMonitor {
    inner: Mutex<Inner>,
    /// One sender per live subscription (`REQ_0847`). Dead
    /// subscribers (dropped receivers) are pruned on each broadcast.
    subscribers: Mutex<Vec<Sender<HealthEvent>>>,
    degraded_due_to_drops: AtomicBool,
}

#[derive(Debug)]
struct Inner {
    aggregate: HealthMonitor,
    ifaces: HashMap<CanIface, IfaceState>,
}

impl CanHealthMonitor {
    /// Construct a monitor with the configured iface set; each iface
    /// starts in `Connecting`.
    #[must_use]
    pub fn new(ifaces: &[CanIface]) -> Self {
        let mut map = HashMap::with_capacity(ifaces.len());
        for &iface in ifaces {
            map.insert(
                iface,
                IfaceState {
                    kind: IfaceHealthKind::Connecting,
                    last_error_at: None,
                },
            );
        }
        Self {
            inner: Mutex::new(Inner {
                aggregate: HealthMonitor::new(),
                ifaces: map,
            }),
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
            .expect("can health subscriber list lock not poisoned");
        subs.retain(|tx| tx.send(event.clone()).is_ok());
    }

    /// Snapshot the externally-visible aggregated state.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex was poisoned by a prior panic.
    pub fn current(&self) -> ConnectorHealth {
        self.inner
            .lock()
            .expect("can health monitor lock not poisoned")
            .aggregate
            .current()
            .clone()
    }

    /// Snapshot one iface's sub-state, or `None` when the iface was
    /// not registered.
    pub fn iface_kind(&self, iface: &CanIface) -> Option<IfaceHealthKind> {
        self.inner
            .lock()
            .expect("can health monitor lock not poisoned")
            .ifaces
            .get(iface)
            .map(|s| s.kind)
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
    /// Panics if the subscriber-list mutex was poisoned.
    #[must_use]
    pub fn subscribe(&self) -> Receiver<HealthEvent> {
        let (tx, rx) = unbounded();
        self.subscribers
            .lock()
            .expect("can health subscriber list lock not poisoned")
            .push(tx);
        rx
    }

    /// Set one iface's sub-state and recompute the aggregate. Emits a
    /// `HealthEvent` only when the aggregate transitions.
    ///
    /// # Errors
    ///
    /// Returns [`CanHealthError::Illegal`] when the recomputed
    /// aggregate transition is illegal per `ARCH_0012` (should not
    /// happen under correct dispatcher operation, but guards against
    /// bugs).
    pub fn set_iface(
        &self,
        iface: CanIface,
        kind: IfaceHealthKind,
    ) -> Result<Option<HealthEvent>, CanHealthError> {
        let mut guard = self
            .inner
            .lock()
            .expect("can health monitor lock not poisoned");
        let entry = guard.ifaces.entry(iface).or_insert(IfaceState {
            kind,
            last_error_at: None,
        });
        entry.kind = kind;
        if matches!(kind, IfaceHealthKind::Degraded | IfaceHealthKind::Down) {
            entry.last_error_at = Some(Instant::now());
        }
        let target = aggregate(&guard.ifaces, iface);
        let current_kind = guard.aggregate.current().kind();
        if current_kind == target.kind() {
            return Ok(None);
        }
        let evt = guard
            .aggregate
            .try_transition_to(target)
            .map_err(CanHealthError::Illegal)?;
        // Recovery to Up via the iface aggregator re-arms the
        // drops latch (`REQ_0608`).
        if evt.to.kind() == ConnectorHealthKind::Up {
            self.degraded_due_to_drops.store(false, Ordering::Release);
        }
        self.broadcast(&evt);
        Ok(Some(evt))
    }

    /// Record a cumulative inbound-drop count from one channel's
    /// [`crate::InboundOutcome::Dropped`] return. When `count` crosses
    /// the supplied `threshold` AND the drops-latch is unset AND the
    /// aggregator is currently `Up`, emit a single
    /// `Up → Degraded { reason: "dropped N inbound frames" }` transition
    /// and set the latch (`REQ_0608`).
    ///
    /// Returns the emitted `HealthEvent` when a transition actually
    /// fired, or `None` if the latch was already set, the threshold has
    /// not been crossed, or the aggregator is not in `Up`.
    pub fn record_inbound_drop(&self, count: u64, threshold: u64) -> Option<HealthEvent> {
        if count < threshold || self.degraded_due_to_drops.load(Ordering::Acquire) {
            return None;
        }
        let mut guard = self
            .inner
            .lock()
            .expect("can health monitor lock not poisoned");
        if guard.aggregate.current().kind() != ConnectorHealthKind::Up {
            // Already Degraded / Down for another reason — the spec
            // says "skip emitting" (Degraded state is already there).
            // Latch so we don't re-check on every dropped frame.
            self.degraded_due_to_drops.store(true, Ordering::Release);
            return None;
        }
        let target = ConnectorHealth::Degraded {
            reason: format!("dropped {count} inbound frames"),
        };
        let evt = guard.aggregate.try_transition_to(target).ok()?;
        self.degraded_due_to_drops.store(true, Ordering::Release);
        self.broadcast(&evt);
        Some(evt)
    }

    /// Test helper: snapshot the drops-latch state.
    #[must_use]
    pub fn degraded_due_to_drops(&self) -> bool {
        self.degraded_due_to_drops.load(Ordering::Acquire)
    }
}

/// Failure modes of [`CanHealthMonitor::set_iface`].
#[derive(Debug, thiserror::Error)]
pub enum CanHealthError {
    /// Aggregate transition is illegal per `ARCH_0012`.
    #[error(transparent)]
    Illegal(#[from] IllegalTransition),
}

impl From<CanHealthError> for ConnectorError {
    fn from(err: CanHealthError) -> Self {
        match err {
            CanHealthError::Illegal(e) => Self::stack(e),
        }
    }
}

/// Compute the aggregate `ConnectorHealth` from the per-iface map.
///
/// `triggering_iface` is the iface whose sub-state just changed —
/// used to populate `Degraded`'s `reason` and `Down`'s `reason` with
/// the offending iface name (`REQ_0635`'s aggregation rule).
fn aggregate(
    ifaces: &HashMap<CanIface, IfaceState>,
    triggering_iface: CanIface,
) -> ConnectorHealth {
    if ifaces.is_empty() {
        return ConnectorHealth::Connecting {
            since: Instant::now(),
        };
    }
    let mut up = 0usize;
    let mut degraded = 0usize;
    let mut down = 0usize;
    for s in ifaces.values() {
        match s.kind {
            IfaceHealthKind::Up => up += 1,
            IfaceHealthKind::Connecting => {}
            IfaceHealthKind::Degraded => degraded += 1,
            IfaceHealthKind::Down => down += 1,
        }
    }
    let total = ifaces.len();
    // All down → connector Down (REQ_0630).
    if down == total {
        return ConnectorHealth::Down {
            reason: format!("all ifaces down (latest: {triggering_iface})"),
            since: Instant::now(),
        };
    }
    // At least one Up needed before we leave Connecting.
    if up == 0 && degraded == 0 {
        // All Connecting or mix of Connecting + Down — still bringing up.
        return ConnectorHealth::Connecting {
            since: Instant::now(),
        };
    }
    // Any degraded or any down (with some up) → connector Degraded.
    if degraded > 0 || down > 0 {
        return ConnectorHealth::Degraded {
            reason: format!("iface {triggering_iface} sub-state degraded or down"),
        };
    }
    // up + connecting only, with up >= 1 → Up.
    ConnectorHealth::Up
}

#[cfg(test)]
mod tests {
    use super::*;
    use taktora_connector_core::ConnectorHealthKind;

    fn iface(name: &str) -> CanIface {
        CanIface::new(name).unwrap()
    }

    #[test]
    fn fresh_monitor_is_connecting() {
        let m = CanHealthMonitor::new(&[iface("vcan0"), iface("vcan1")]);
        assert_eq!(m.current().kind(), ConnectorHealthKind::Connecting);
    }

    #[test]
    fn worst_of_two_ifaces() {
        let a = iface("vcan0");
        let b = iface("vcan1");
        let m = CanHealthMonitor::new(&[a, b]);
        let _ = m.set_iface(a, IfaceHealthKind::Up).unwrap();
        let _ = m.set_iface(b, IfaceHealthKind::Up).unwrap();
        assert_eq!(m.current().kind(), ConnectorHealthKind::Up);

        // One iface down while the other is up → aggregate Degraded.
        let evt = m.set_iface(a, IfaceHealthKind::Down).unwrap().unwrap();
        assert_eq!(evt.to.kind(), ConnectorHealthKind::Degraded);
        assert_eq!(m.current().kind(), ConnectorHealthKind::Degraded);

        // Both down → Down.
        let evt = m.set_iface(b, IfaceHealthKind::Down).unwrap().unwrap();
        assert_eq!(evt.to.kind(), ConnectorHealthKind::Down);
    }

    #[test]
    fn subscriber_receives_aggregate_events() {
        let a = iface("vcan0");
        let m = CanHealthMonitor::new(&[a]);
        let sub = m.subscribe();
        let _ = m.set_iface(a, IfaceHealthKind::Up).unwrap();
        let evt = sub.try_recv().unwrap();
        assert_eq!(evt.to.kind(), ConnectorHealthKind::Up);
    }

    /// `REQ_0608`: when cumulative drops cross the threshold and the
    /// aggregator is `Up`, a single `Up → Degraded` is emitted; the
    /// next `→ Up` transition re-arms the latch.
    #[test]
    fn record_inbound_drop_emits_degraded_once() {
        let a = iface("vcan0");
        let m = CanHealthMonitor::new(&[a]);
        let _ = m.set_iface(a, IfaceHealthKind::Up).unwrap();
        let sub = m.subscribe();
        // Drain the bring-up event so the test asserts only the drops-
        // driven transition.
        let _ = sub.try_recv();

        // First drop crossing the threshold (1) emits Degraded.
        let evt = m
            .record_inbound_drop(1, 1)
            .expect("first cross-threshold emits Degraded");
        assert_eq!(evt.from.kind(), ConnectorHealthKind::Up);
        assert_eq!(evt.to.kind(), ConnectorHealthKind::Degraded);
        match &evt.to {
            ConnectorHealth::Degraded { reason } => {
                assert!(
                    reason.contains("dropped") && reason.contains('1'),
                    "reason {reason:?} must mention dropped count"
                );
            }
            other => panic!("expected Degraded, got {other:?}"),
        }
        assert!(m.degraded_due_to_drops());

        // Subsequent drops at higher counts are latched out.
        assert!(m.record_inbound_drop(2, 1).is_none());
        assert!(m.record_inbound_drop(3, 1).is_none());

        // Recovery to Up via the iface aggregator re-arms the latch.
        // The aggregate currently sits in `Degraded` (drops-driven), so
        // setting the iface back to `Up` aggregates to `Up` and the
        // legal `Degraded → Up` edge fires.
        let _ = m.set_iface(a, IfaceHealthKind::Up).unwrap();
        assert!(!m.degraded_due_to_drops());
    }
}
