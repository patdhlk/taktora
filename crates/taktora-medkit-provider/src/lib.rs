//! The data-source seam for `taktora-medkit`.
//!
//! The diagnostic gateway reads the system through the [`Provider`] trait, never
//! touching taktora directly. Live data arrives only via binding crates that
//! implement this trait by draining non-blocking taktora callbacks off the
//! control path (`REQ_0910`, `REQ_0913`); this crate also ships a [`MockProvider`]
//! for tests and the walking skeleton.
//!
//! This crate carries **zero** taktora dependencies, holding the
//! extractable-core invariant (`REQ_0916`, `ADR_0111`).

use std::collections::HashMap;

use taktora_medkit_model::{Dtc, Entity, Health};

/// The data-source seam the gateway reads through.
///
/// Implementations must be cheap and non-blocking to call: the gateway may poll
/// them from its request path, so an implementation backed by live taktora data
/// is expected to serve from an already-forwarded snapshot rather than reaching
/// onto the control path (`REQ_0910`).
pub trait Provider: Send + Sync {
    /// Every entity currently known to the diagnostic surface.
    fn entities(&self) -> Vec<Entity>;

    /// The active faults reported against the entity `entity_id`.
    fn faults(&self, entity_id: &str) -> Vec<Dtc>;

    /// The directly-observed (non-rolled-up) health of `entity_id`.
    ///
    /// Returns [`Health::Ok`] for an unknown entity; the worst-wins rollup over
    /// descendants is the gateway's job (`REQ_0912`).
    fn health(&self, entity_id: &str) -> Health;
}

/// An in-memory [`Provider`] for tests and the walking skeleton.
#[derive(Clone, Debug, Default)]
pub struct MockProvider {
    entities: Vec<Entity>,
    faults: HashMap<String, Vec<Dtc>>,
    health: HashMap<String, Health>,
}

impl MockProvider {
    /// Create an empty mock provider.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an entity to the tree.
    #[must_use]
    pub fn with_entity(mut self, entity: Entity) -> Self {
        self.entities.push(entity);
        self
    }

    /// Attach a fault to an entity and record its directly-observed health.
    #[must_use]
    pub fn with_fault(mut self, entity_id: impl Into<String>, dtc: Dtc) -> Self {
        let id = entity_id.into();
        let health = severity_to_health(dtc.severity);
        self.faults.entry(id.clone()).or_default().push(dtc);
        let slot = self.health.entry(id).or_insert(Health::Ok);
        *slot = (*slot).max(health);
        self
    }
}

impl Provider for MockProvider {
    fn entities(&self) -> Vec<Entity> {
        self.entities.clone()
    }

    fn faults(&self, entity_id: &str) -> Vec<Dtc> {
        self.faults.get(entity_id).cloned().unwrap_or_default()
    }

    fn health(&self, entity_id: &str) -> Health {
        self.health.get(entity_id).copied().unwrap_or(Health::Ok)
    }
}

/// Map a fault [`Severity`](taktora_medkit_model::Severity) to the [`Health`] it
/// implies for an entity carrying it.
#[must_use]
pub const fn severity_to_health(severity: taktora_medkit_model::Severity) -> Health {
    use taktora_medkit_model::Severity;
    match severity {
        Severity::Info => Health::Ok,
        Severity::Warn => Health::Warning,
        Severity::Error => Health::Error,
        Severity::Critical => Health::Critical,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use taktora_medkit_model::{
        DtcStatus, EntityKind, EnvironmentData, ExtendedDataRecords, Severity,
    };

    fn dtc(code: &str, severity: Severity) -> Dtc {
        Dtc {
            fault_code: code.to_owned(),
            status: DtcStatus {
                aggregated_status: "CONFIRMED".to_owned(),
                test_failed: true,
                confirmed_dtc: true,
                pending_dtc: false,
            },
            severity,
            occurrence_count: 1,
            reporting_sources: vec![],
            environment_data: EnvironmentData {
                extended_data_records: ExtendedDataRecords {
                    first_occurrence_ns: 0,
                    last_occurrence_ns: 0,
                },
                snapshots: vec![],
            },
        }
    }

    #[test]
    fn mock_returns_entities_and_faults() {
        let provider = MockProvider::new()
            .with_entity(Entity {
                id: "component:nav".to_owned(),
                name: "nav".to_owned(),
                kind: EntityKind::Component,
                parent_id: None,
            })
            .with_fault("component:nav", dtc("STUCK", Severity::Error));

        assert_eq!(provider.entities().len(), 1);
        assert_eq!(provider.faults("component:nav").len(), 1);
        assert_eq!(provider.faults("missing").len(), 0);
        assert_eq!(provider.health("component:nav"), Health::Error);
        assert_eq!(provider.health("missing"), Health::Ok);
    }
}
