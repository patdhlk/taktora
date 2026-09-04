//! Transport-neutral read-diagnostic core for `taktora-medkit`.
//!
//! [`Gateway`] resolves the read families of the SOVD surface — the entity
//! tree, fault lists, and the worst-wins health rollup (`REQ_0912`) — over any
//! [`Provider`], independent of any HTTP framework. The axum surface in
//! `taktora-medkit-gateway-axum` is a thin adapter over this core.
//!
//! Zero taktora dependencies (`REQ_0916`, `ADR_0111`).
//!
//! The HTTP read families are resolved by the pure functions in [`view`]: a
//! [`MergePipeline`] folds provider snapshots into a [`MergedView`], and the
//! resolver methods on the view turn a request into a wire DTO (`REQ_0917`).

use std::collections::HashMap;

use taktora_medkit_model::{Collection, Entity, FaultSummary, Health};
use taktora_medkit_provider::Provider;

pub mod view;

pub use taktora_medkit_manifest::Manifest;
pub use view::{
    API_BASE, FaultStatusFilter, MergePipeline, MergedView, ResolveError, SOVD_VERSION,
    collection_segment, type_singular,
};

/// The transport-neutral read-diagnostic core.
#[derive(Clone, Debug)]
pub struct Gateway<P> {
    provider: P,
    manifest: Option<Manifest>,
}

impl<P: Provider> Gateway<P> {
    /// Build a gateway reading through `provider`, with flat grouping (no
    /// manifest).
    pub const fn new(provider: P) -> Self {
        Self {
            provider,
            manifest: None,
        }
    }

    /// Attach the grouping [`Manifest`] the view is folded through, re-parenting
    /// the provider's raw entities into the declared Area/Component skeleton
    /// (`REQ_0921`). An empty manifest leaves grouping flat (`REQ_0922`).
    #[must_use]
    pub fn with_manifest(mut self, manifest: Manifest) -> Self {
        self.manifest = Some(manifest);
        self
    }

    /// Borrow the underlying provider.
    pub const fn provider(&self) -> &P {
        &self.provider
    }

    /// Build the merged read-model the HTTP resolvers serve from, by folding
    /// the provider's current snapshot (and the attached manifest, if any)
    /// through the [`MergePipeline`].
    #[must_use]
    pub fn view(&self) -> MergedView {
        MergedView::from_snapshot_with_manifest(self.provider.snapshot(), self.manifest.clone())
    }

    /// The full entity tree, as a collection envelope.
    pub fn entities(&self) -> Collection<Entity> {
        Collection::new(self.provider.entities())
    }

    /// The active faults for `entity_id`, as a collection envelope.
    pub fn faults(&self, entity_id: &str) -> Collection<FaultSummary> {
        Collection::new(self.provider.faults(entity_id))
    }

    /// The worst-wins aggregated health of `entity_id`: the worst of its own
    /// directly-observed health and the rolled-up health of every descendant
    /// (`REQ_0912`).
    ///
    /// An entity with no children rolls up to its own health; an unknown entity
    /// rolls up to [`Health::Ok`].
    pub fn rolled_up_health(&self, entity_id: &str) -> Health {
        let entities = self.provider.entities();
        let children = children_index(&entities);
        self.roll_up(entity_id, &children)
    }

    fn roll_up(&self, entity_id: &str, children: &HashMap<&str, Vec<&str>>) -> Health {
        let mut worst = self.provider.health(entity_id);
        if let Some(kids) = children.get(entity_id) {
            for child in kids {
                worst = worst.max(self.roll_up(child, children));
            }
        }
        worst
    }
}

/// Index entities by parent id, yielding each parent's list of child ids.
fn children_index(entities: &[Entity]) -> HashMap<&str, Vec<&str>> {
    let mut index: HashMap<&str, Vec<&str>> = HashMap::new();
    for entity in entities {
        if let Some(parent) = entity.parent_id.as_deref() {
            index.entry(parent).or_default().push(entity.id.as_str());
        }
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;
    use taktora_medkit_model::{EntityKind, Severity};
    use taktora_medkit_provider::MockProvider;

    fn entity(id: &str, kind: EntityKind, parent: Option<&str>) -> Entity {
        Entity {
            href: format!("/api/v1/{id}"),
            id: id.to_owned(),
            name: id.to_owned(),
            kind,
            parent_id: parent.map(ToOwned::to_owned),
            description: None,
            x_medkit: None,
        }
    }

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

    fn tree() -> MockProvider {
        // area:root -> component:nav -> app:planner, app:controller
        MockProvider::new()
            .with_entity(entity("area:root", EntityKind::Area, None))
            .with_entity(entity(
                "component:nav",
                EntityKind::Component,
                Some("area:root"),
            ))
            .with_entity(entity(
                "app:planner",
                EntityKind::App,
                Some("component:nav"),
            ))
            .with_entity(entity(
                "app:controller",
                EntityKind::App,
                Some("component:nav"),
            ))
    }

    /// `TEST_0902` — the gateway resolves the entity tree and fault lists over the
    /// mock provider with no HTTP layer.
    // @need-ids: TEST_0902
    #[test]
    fn resolves_entities_and_faults_over_mock() {
        let provider = tree().with_fault("app:planner", fault("STUCK", Severity::Error));
        let gateway = Gateway::new(provider);

        assert_eq!(gateway.entities().x_medkit.total_count, 4);
        assert_eq!(gateway.faults("app:planner").items.len(), 1);
        assert_eq!(gateway.faults("app:controller").items.len(), 0);
    }

    /// `TEST_0903` — worst-wins health rolls a faulting leaf up to its ancestors.
    // @need-ids: TEST_0903
    #[test]
    fn health_rolls_up_worst_wins() {
        let provider = tree()
            .with_fault("app:planner", fault("WARMISH", Severity::Warn))
            .with_fault("app:controller", fault("STUCK", Severity::Error));
        let gateway = Gateway::new(provider);

        // Leaf reflects its own fault.
        assert_eq!(gateway.rolled_up_health("app:planner"), Health::Warning);
        // Component is the worst of its two children (Warning vs Error).
        assert_eq!(gateway.rolled_up_health("component:nav"), Health::Error);
        // Root inherits the worst in the whole tree.
        assert_eq!(gateway.rolled_up_health("area:root"), Health::Error);
    }

    /// `TEST_0903` — a healthy subtree rolls up to `Ok`; an unknown id is `Ok`.
    // @need-ids: TEST_0903
    #[test]
    fn healthy_tree_and_unknown_roll_up_ok() {
        let gateway = Gateway::new(tree());
        assert_eq!(gateway.rolled_up_health("area:root"), Health::Ok);
        assert_eq!(gateway.rolled_up_health("nope"), Health::Ok);
    }
}
