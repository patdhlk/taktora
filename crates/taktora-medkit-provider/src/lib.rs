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

use std::collections::{BTreeMap, HashMap};

use serde_json::Value;
use taktora_medkit_model::{Entity, EnvironmentData, FaultSummary, Health, Severity};

pub mod action;

pub use action::{
    ActionError, ActionSink, BulkCategory, BulkDescriptor, ConfigEntry, Execution, ExecutionStatus,
    OperationDef, ResourceRef, ScriptDef, SimActionSink,
};

/// A typed relationship between two SOVD entities, as the contract exposes them
/// under relationship sub-resources (`…/hosts`, `…/depends-on`, …).
///
/// The wire `segment` is the URL path component and the relation's identity; the
/// back-link key under a relationship envelope's `_links` is the *parent's*
/// entity-type singular, except [`Relation::Subcomponents`], which the contract
/// keys as `parent`.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum Relation {
    /// An area's directly-contained children (`/contains`).
    Contains,
    /// The components grouped under an area (`/components`).
    Components,
    /// The subcomponents of a component (`/subcomponents`).
    Subcomponents,
    /// The apps a component or function hosts (`/hosts`).
    Hosts,
    /// The entities this one depends on (`/depends-on`).
    DependsOn,
    /// The component an app is located on (`/is-located-on`).
    IsLocatedOn,
    /// The function an app belongs to (`/belongs-to`).
    BelongsTo,
}

impl Relation {
    /// Every relation, for exhaustive route registration.
    pub const ALL: [Self; 7] = [
        Self::Contains,
        Self::Components,
        Self::Subcomponents,
        Self::Hosts,
        Self::DependsOn,
        Self::IsLocatedOn,
        Self::BelongsTo,
    ];

    /// The URL path segment that names this relation (e.g. `is-located-on`).
    #[must_use]
    pub const fn segment(self) -> &'static str {
        match self {
            Self::Contains => "contains",
            Self::Components => "components",
            Self::Subcomponents => "subcomponents",
            Self::Hosts => "hosts",
            Self::DependsOn => "depends-on",
            Self::IsLocatedOn => "is-located-on",
            Self::BelongsTo => "belongs-to",
        }
    }

    /// Parse a relation from its wire path segment.
    #[must_use]
    pub fn from_segment(segment: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.segment() == segment)
    }
}

/// One directed relationship edge plus the already-wire-shaped summary item to
/// emit for the target entity under the parent's relationship sub-resource.
///
/// The producer (the mock here, a binding's snapshotting later) owns the item's
/// `x-medkit` decoration, since the right decoration is context-dependent (a
/// `…/hosts` app item omits the `component_id` that the top-level `/apps` item
/// carries, and an `…/is-located-on` item carries no `x-medkit` at all).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelationshipEdge {
    /// The id of the parent entity the relationship hangs off.
    pub from_id: String,
    /// Which relationship this edge realises.
    pub relation: Relation,
    /// The wire-shaped summary item for the related entity.
    pub item: Entity,
}

/// A point-in-time, self-consistent read-model the gateway serves from.
///
/// This is the **snapshot contract**: the shape a binding's snapshotting
/// produces off the control path (GitHub #83/#84) and the merge pipeline
/// consumes (#82). It is plain data — no behaviour, no taktora types — so it can
/// be assembled once and read concurrently.
#[derive(Clone, Debug, Default)]
pub struct ProviderSnapshot {
    /// Every entity in the tree, in the order top-level lists should present it.
    pub entities: Vec<Entity>,
    /// The relationship edges between entities.
    pub relationships: Vec<RelationshipEdge>,
    /// Faults keyed by the entity id they are scoped to.
    pub faults: BTreeMap<String, Vec<FaultSummary>>,
    /// Per-fault freeze-frame environment data, keyed `entity_id → fault_code`.
    ///
    /// Additive seam (`ADR_0116`, `REQ_0929`): a binding that captures
    /// freeze-frames populates this so the gateway's `…/faults/{code}` detail can
    /// carry the real `snapshots` / `extended_data_records`. The `FaultSummary`
    /// list path is unchanged; the map is empty for providers that capture none.
    pub fault_environments: BTreeMap<String, BTreeMap<String, EnvironmentData<Value>>>,
    /// Readable data trees keyed by entity id (served under `…/data`).
    pub data: BTreeMap<String, Value>,
}

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
    fn faults(&self, entity_id: &str) -> Vec<FaultSummary>;

    /// The freeze-frame environment data captured for fault `fault_code` on
    /// `entity_id`, if the provider captured any (`REQ_0929`).
    ///
    /// Defaults to `None`: a provider that carries no freeze-frames need not
    /// implement this, and the gateway falls back to the occurrence-only detail
    /// shape. A capturing binding either overrides this or populates
    /// [`ProviderSnapshot::fault_environments`] directly.
    fn fault_environment(
        &self,
        entity_id: &str,
        fault_code: &str,
    ) -> Option<EnvironmentData<Value>> {
        let _ = (entity_id, fault_code);
        None
    }

    /// The directly-observed (non-rolled-up) health of `entity_id`.
    ///
    /// Returns [`Health::Ok`] for an unknown entity; the worst-wins rollup over
    /// descendants is the gateway's job (`REQ_0912`).
    fn health(&self, entity_id: &str) -> Health;

    /// A consistent read-model snapshot the gateway serves a request batch from.
    ///
    /// The default assembles entities and their faults from the per-entity
    /// methods, carrying no relationships or data; a richer source (the mock, or
    /// a live binding) overrides this to populate the relationship graph and
    /// readable data in one consistent read.
    fn snapshot(&self) -> ProviderSnapshot {
        let entities = self.entities();
        let faults = entities
            .iter()
            .filter_map(|e| {
                let faults = self.faults(&e.id);
                (!faults.is_empty()).then(|| (e.id.clone(), faults))
            })
            .collect();
        ProviderSnapshot {
            entities,
            relationships: Vec::new(),
            faults,
            fault_environments: BTreeMap::new(),
            data: BTreeMap::new(),
        }
    }
}

/// An in-memory [`Provider`] for tests and the walking skeleton.
#[derive(Clone, Debug, Default)]
pub struct MockProvider {
    entities: Vec<Entity>,
    faults: HashMap<String, Vec<FaultSummary>>,
    fault_environments: BTreeMap<String, BTreeMap<String, EnvironmentData<Value>>>,
    health: HashMap<String, Health>,
    relationships: Vec<RelationshipEdge>,
    data: BTreeMap<String, Value>,
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
    pub fn with_fault(mut self, entity_id: impl Into<String>, fault: FaultSummary) -> Self {
        let id = entity_id.into();
        let health =
            Severity::from_wire_value(fault.severity).map_or(Health::Ok, severity_to_health);
        self.faults.entry(id.clone()).or_default().push(fault);
        let slot = self.health.entry(id).or_insert(Health::Ok);
        *slot = (*slot).max(health);
        self
    }

    /// Record a relationship edge from `from_id` to the wire-shaped `item`.
    #[must_use]
    pub fn with_relationship(
        mut self,
        from_id: impl Into<String>,
        relation: Relation,
        item: Entity,
    ) -> Self {
        self.relationships.push(RelationshipEdge {
            from_id: from_id.into(),
            relation,
            item,
        });
        self
    }

    /// Attach a readable data tree to an entity (served under `…/data`).
    #[must_use]
    pub fn with_data(mut self, entity_id: impl Into<String>, data: Value) -> Self {
        self.data.insert(entity_id.into(), data);
        self
    }

    /// Attach freeze-frame environment data for a fault on an entity, surfaced by
    /// the gateway under `…/faults/{fault_code}` (`REQ_0929`).
    #[must_use]
    pub fn with_fault_environment(
        mut self,
        entity_id: impl Into<String>,
        fault_code: impl Into<String>,
        env: EnvironmentData<Value>,
    ) -> Self {
        self.fault_environments
            .entry(entity_id.into())
            .or_default()
            .insert(fault_code.into(), env);
        self
    }
}

impl Provider for MockProvider {
    fn entities(&self) -> Vec<Entity> {
        self.entities.clone()
    }

    fn faults(&self, entity_id: &str) -> Vec<FaultSummary> {
        self.faults.get(entity_id).cloned().unwrap_or_default()
    }

    fn fault_environment(
        &self,
        entity_id: &str,
        fault_code: &str,
    ) -> Option<EnvironmentData<Value>> {
        self.fault_environments
            .get(entity_id)
            .and_then(|m| m.get(fault_code))
            .cloned()
    }

    fn health(&self, entity_id: &str) -> Health {
        self.health.get(entity_id).copied().unwrap_or(Health::Ok)
    }

    fn snapshot(&self) -> ProviderSnapshot {
        ProviderSnapshot {
            entities: self.entities.clone(),
            relationships: self.relationships.clone(),
            faults: self
                .faults
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            fault_environments: self.fault_environments.clone(),
            data: self.data.clone(),
        }
    }
}

/// Map a fault [`Severity`] to the [`Health`] it implies for an entity carrying
/// it.
#[must_use]
pub const fn severity_to_health(severity: Severity) -> Health {
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
    use taktora_medkit_model::EntityKind;

    fn fault(code: &str, severity: Severity) -> FaultSummary {
        FaultSummary {
            description: code.to_owned(),
            fault_code: code.to_owned(),
            first_occurred: 0.0,
            last_occurred: 0.0,
            occurrence_count: 1,
            reporting_sources: vec![],
            severity: severity.wire_value(),
            severity_label: format!("{severity:?}"),
            status: "CONFIRMED".to_owned(),
        }
    }

    #[test]
    fn mock_returns_entities_and_faults() {
        let provider = MockProvider::new()
            .with_entity(Entity {
                href: "/api/v1/components/nav".to_owned(),
                id: "component:nav".to_owned(),
                name: "nav".to_owned(),
                kind: EntityKind::Component,
                parent_id: None,
                description: None,
                x_medkit: None,
            })
            .with_fault("component:nav", fault("STUCK", Severity::Error));

        assert_eq!(provider.entities().len(), 1);
        assert_eq!(provider.faults("component:nav").len(), 1);
        assert_eq!(provider.faults("missing").len(), 0);
        assert_eq!(provider.health("component:nav"), Health::Error);
        assert_eq!(provider.health("missing"), Health::Ok);
    }
}
