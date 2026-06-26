//! [`PublishHealth`]: the connector's local publish-health state machine
//! (`REQ_0883`).
//!
//! The UI connector has no remote bus partner, so its health reflects **local**
//! publishing only: the pump running maps to [`ConnectorHealth::Up`]; publish
//! back-pressure or drops map to [`ConnectorHealth::Degraded`]; a recovered
//! publish returns to [`ConnectorHealth::Up`]. Crucially, the **absence of subscribers is not a
//! fault** — a UI that simply has not attached yet must not flip the connector
//! to degraded.
//!
//! This wraps the shared [`HealthMonitor`] so the transitions are gated by the
//! same `ARCH_0012` state machine as every other connector, and is `Clone`
//! (`Arc`-backed) so the pump thread can drive it while the connector reads the
//! current state.

use std::sync::{Arc, Mutex};

use taktora_connector_core::ConnectorError;
use taktora_connector_core::health::{ConnectorHealth, HealthMonitor};

/// Local publish-health for the UI connector.
///
/// Clone-able: every clone shares one [`HealthMonitor`], so the pump can
/// [`observe`](Self::observe) publish outcomes on its thread while the connector
/// reports [`current`](Self::current).
#[derive(Clone)]
pub struct PublishHealth {
    inner: Arc<Mutex<HealthMonitor>>,
}

impl Default for PublishHealth {
    fn default() -> Self {
        Self::new()
    }
}

impl PublishHealth {
    /// A fresh monitor in the initial `Connecting` state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HealthMonitor::new())),
        }
    }

    /// Mark the pump as running: transition to [`ConnectorHealth::Up`].
    ///
    /// A no-op if already `Up` (same-discriminant transitions are illegal under
    /// `ARCH_0012`, so they are silently debounced here).
    pub fn mark_running(&self) {
        self.transition_up();
    }

    /// React to one publish outcome.
    ///
    /// `Ok` recovers to [`ConnectorHealth::Up`]; an error (notably
    /// [`ConnectorError::BackPressure`]) degrades to
    /// [`Degraded`](ConnectorHealth::Degraded). Both are debounced: repeated
    /// identical outcomes do not churn the state.
    pub fn observe(&self, outcome: &Result<(), ConnectorError>) {
        match outcome {
            Ok(()) => self.transition_up(),
            Err(err) => self.degrade(&err.to_string()),
        }
    }

    /// Degrade with an explicit reason (e.g. the pump observed dropped publishes
    /// this tick). Debounced if already degraded.
    pub fn degrade(&self, reason: &str) {
        let mut monitor = self.inner.lock().expect("health lock");
        if matches!(monitor.current(), ConnectorHealth::Degraded { .. }) {
            return;
        }
        // `Up`/`Connecting` -> `Degraded` are both legal; ignore an illegal
        // attempt (only reachable from `Down`, which this connector never enters
        // locally).
        let _ = monitor.try_transition_to(ConnectorHealth::Degraded {
            reason: reason.to_owned(),
        });
    }

    /// The current health state.
    #[must_use]
    pub fn current(&self) -> ConnectorHealth {
        self.inner.lock().expect("health lock").current().clone()
    }

    fn transition_up(&self) {
        let mut monitor = self.inner.lock().expect("health lock");
        if matches!(monitor.current(), ConnectorHealth::Up) {
            return;
        }
        let _ = monitor.try_transition_to(ConnectorHealth::Up);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use taktora_connector_core::health::ConnectorHealthKind;

    fn kind(h: &PublishHealth) -> ConnectorHealthKind {
        h.current().kind()
    }

    #[test]
    fn starts_connecting() {
        let h = PublishHealth::new();
        assert_eq!(kind(&h), ConnectorHealthKind::Connecting);
    }

    #[test]
    fn pump_running_goes_up() {
        let h = PublishHealth::new();
        h.mark_running();
        assert_eq!(kind(&h), ConnectorHealthKind::Up);
    }

    #[test]
    fn backpressure_degrades_then_recovers() {
        let h = PublishHealth::new();
        h.mark_running();
        assert_eq!(kind(&h), ConnectorHealthKind::Up);

        h.observe(&Err(ConnectorError::BackPressure));
        assert_eq!(kind(&h), ConnectorHealthKind::Degraded);

        h.observe(&Ok(()));
        assert_eq!(kind(&h), ConnectorHealthKind::Up);
    }

    #[test]
    fn repeated_degrade_is_debounced() {
        let h = PublishHealth::new();
        h.mark_running();
        h.observe(&Err(ConnectorError::BackPressure));
        // A second drop must not attempt an illegal Degraded -> Degraded
        // transition (which would panic via `transition_to`).
        h.observe(&Err(ConnectorError::BackPressure));
        assert_eq!(kind(&h), ConnectorHealthKind::Degraded);
    }

    #[test]
    fn repeated_ok_is_debounced() {
        let h = PublishHealth::new();
        h.mark_running();
        h.observe(&Ok(()));
        h.observe(&Ok(()));
        assert_eq!(kind(&h), ConnectorHealthKind::Up);
    }

    #[test]
    fn subscriber_absence_is_not_a_fault() {
        // There is no API to report "no subscribers" as a fault: the pump simply
        // skips zero-subscriber entries and never calls `observe`/`degrade`, so
        // health stays wherever it was. A pump running with no subscribers
        // remains `Up`.
        let h = PublishHealth::new();
        h.mark_running();
        // Simulate several ticks where every entry was skipped (no observe call).
        assert_eq!(kind(&h), ConnectorHealthKind::Up);
    }

    #[test]
    fn clones_share_state() {
        let h = PublishHealth::new();
        let pump_side = h.clone();
        pump_side.mark_running();
        assert_eq!(kind(&h), ConnectorHealthKind::Up);
    }
}
