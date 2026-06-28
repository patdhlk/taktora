//! The merged read-model and the pure read-family resolvers over it.
//!
//! [`MergePipeline`] folds one or more [`ProviderSnapshot`]s (and, in a later
//! slice, a manifest) into a single [`MergedView`]; the resolver methods on
//! [`MergedView`] are pure `(&MergedView, params) -> wire-type` functions the
//! HTTP layer calls. Keeping them pure and transport-neutral lets the same
//! resolvers run over a mock, a manifest, or live taktora bindings (`REQ_0916`).

use std::collections::{BTreeMap, HashMap};

use serde_json::{Value, json};
use taktora_medkit_model::Entity;
use taktora_medkit_model::{
    Collection, CollectionMeta, DtcStatus, EntityKind, EnvironmentData, ExtendedDataRecords,
    FaultDetail, FaultDetailMeta, FaultItem, FaultList, FaultListMeta, FaultSummary, GenericError,
};
use taktora_medkit_provider::{ProviderSnapshot, Relation, RelationshipEdge};

/// The API path prefix every served resource hangs off.
pub const API_BASE: &str = "/api/v1";

/// The SOVD/UDS contract version this surface speaks.
pub const SOVD_VERSION: &str = "1.0.0";

/// A resolver failure that maps to a contract-shaped error response.
///
/// Carries the [`GenericError`] envelope the HTTP layer serializes verbatim;
/// every variant is a `404 Not Found` in this read-only skeleton.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolveError {
    /// The addressed entity, fault, or data path does not exist.
    NotFound(GenericError),
}

impl ResolveError {
    fn entity_not_found(entity_id: &str) -> Self {
        Self::NotFound(GenericError {
            error_code: "entity-not-found".to_owned(),
            message: "Entity not found".to_owned(),
            parameters: BTreeMap::from([("entity_id".to_owned(), entity_id.to_owned())]),
        })
    }

    fn fault_not_found(fault_code: &str) -> Self {
        Self::NotFound(GenericError {
            error_code: "fault-not-found".to_owned(),
            message: "Fault not found".to_owned(),
            parameters: BTreeMap::from([("fault_code".to_owned(), fault_code.to_owned())]),
        })
    }

    fn data_not_found(path: &str) -> Self {
        Self::NotFound(GenericError {
            error_code: "data-not-found".to_owned(),
            message: "Data resource not found".to_owned(),
            parameters: BTreeMap::from([("data_id".to_owned(), path.to_owned())]),
        })
    }
}

/// Which lifecycle states a fault list is filtered to (`?status=` query).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum FaultStatusFilter {
    /// Every fault regardless of lifecycle state. The default with no query.
    #[default]
    All,
    /// Pending / pre-failed faults.
    Pending,
    /// Confirmed faults.
    Confirmed,
    /// Cleared faults.
    Cleared,
    /// Healed faults.
    Healed,
}

impl FaultStatusFilter {
    /// Parse the `?status=` query value; `None` (absent) selects [`Self::All`].
    ///
    /// Returns `None` for an unrecognised value so the caller can answer
    /// `400 Bad Request`.
    #[must_use]
    pub fn parse(value: Option<&str>) -> Option<Self> {
        match value {
            None | Some("all") => Some(Self::All),
            Some("pending") => Some(Self::Pending),
            Some("confirmed") => Some(Self::Confirmed),
            Some("cleared") => Some(Self::Cleared),
            Some("healed") => Some(Self::Healed),
            Some(_) => None,
        }
    }

    fn matches(self, status: &str) -> bool {
        match self {
            Self::All => true,
            Self::Pending => status == "PREFAILED" || status == "PENDING",
            Self::Confirmed => status == "CONFIRMED",
            Self::Cleared => status == "CLEARED",
            Self::Healed => status == "HEALED",
        }
    }
}

/// The plural collection segment for an entity kind (`apps`, `components`, …).
#[must_use]
pub const fn collection_segment(kind: EntityKind) -> &'static str {
    match kind {
        EntityKind::Area => "areas",
        EntityKind::Component => "components",
        EntityKind::Function => "functions",
        EntityKind::App => "apps",
    }
}

/// The singular type token for an entity kind (`app`, `component`, …).
#[must_use]
pub const fn type_singular(kind: EntityKind) -> &'static str {
    match kind {
        EntityKind::Area => "area",
        EntityKind::Component => "component",
        EntityKind::Function => "function",
        EntityKind::App => "app",
    }
}

/// Folds provider snapshots (and, later, a manifest) into one [`MergedView`].
///
/// The walking skeleton merges a single mock snapshot — an identity fold — but
/// the seam is shaped for the downstream slices: #82 applies the manifest here,
/// and #83/#84 contribute additional [`ProviderSnapshot`]s to be merged.
#[derive(Clone, Debug, Default)]
pub struct MergePipeline {
    snapshots: Vec<ProviderSnapshot>,
}

impl MergePipeline {
    /// An empty pipeline.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a snapshot to be merged. Later snapshots win on id collision.
    #[must_use]
    pub fn with_snapshot(mut self, snapshot: ProviderSnapshot) -> Self {
        self.snapshots.push(snapshot);
        self
    }

    /// Fold the accumulated snapshots into the merged read-model.
    #[must_use]
    pub fn merge(self) -> MergedView {
        let mut entities: Vec<Entity> = Vec::new();
        let mut by_id: HashMap<String, usize> = HashMap::new();
        let mut relationships: Vec<RelationshipEdge> = Vec::new();
        let mut faults: BTreeMap<String, Vec<FaultSummary>> = BTreeMap::new();
        let mut data: BTreeMap<String, Value> = BTreeMap::new();

        for snapshot in self.snapshots {
            for entity in snapshot.entities {
                if let Some(&idx) = by_id.get(&entity.id) {
                    entities[idx] = entity;
                } else {
                    by_id.insert(entity.id.clone(), entities.len());
                    entities.push(entity);
                }
            }
            relationships.extend(snapshot.relationships);
            faults.extend(snapshot.faults);
            data.extend(snapshot.data);
        }

        MergedView {
            entities,
            by_id,
            relationships,
            faults,
            data,
        }
    }
}

/// A consistent, indexed read-model the resolvers serve from.
#[derive(Clone, Debug, Default)]
pub struct MergedView {
    entities: Vec<Entity>,
    by_id: HashMap<String, usize>,
    relationships: Vec<RelationshipEdge>,
    faults: BTreeMap<String, Vec<FaultSummary>>,
    data: BTreeMap<String, Value>,
}

impl MergedView {
    /// Build a view from a single snapshot (the common skeleton case).
    #[must_use]
    pub fn from_snapshot(snapshot: ProviderSnapshot) -> Self {
        MergePipeline::new().with_snapshot(snapshot).merge()
    }

    /// Look up an entity by id.
    #[must_use]
    pub fn entity(&self, id: &str) -> Option<&Entity> {
        self.by_id.get(id).map(|&idx| &self.entities[idx])
    }

    /// Require an entity of the given `kind`, or a not-found error.
    fn require(&self, kind: EntityKind, id: &str) -> Result<&Entity, ResolveError> {
        match self.entity(id) {
            Some(entity) if entity.kind == kind => Ok(entity),
            _ => Err(ResolveError::entity_not_found(id)),
        }
    }

    /// The top-level collection of entities of a kind (`/areas`, `/apps`, …).
    ///
    /// A top-level list carries `items` + `x-medkit.total_count` and no
    /// `_links`, per the contract envelope.
    #[must_use]
    pub fn list(&self, kind: EntityKind) -> Collection<Entity> {
        Collection::new(
            self.entities
                .iter()
                .filter(|e| e.kind == kind)
                .cloned()
                .collect(),
        )
    }

    /// A relationship sub-resource (`…/hosts`, `…/is-located-on`, …).
    ///
    /// Emits the `items` for the related entities plus the `_links` map the
    /// contract attaches: a `self` link and a back-link keyed by the parent's
    /// type singular (or `parent` for [`Relation::Subcomponents`]).
    ///
    /// # Errors
    ///
    /// [`ResolveError::NotFound`] if no entity of `kind` has id `id`.
    pub fn relationship(
        &self,
        kind: EntityKind,
        id: &str,
        relation: Relation,
    ) -> Result<Collection<Entity>, ResolveError> {
        self.require(kind, id)?;

        let items: Vec<Entity> = self
            .relationships
            .iter()
            .filter(|edge| edge.from_id == id && edge.relation == relation)
            .map(|edge| edge.item.clone())
            .collect();

        let collection = collection_segment(kind);
        let self_link = format!("{API_BASE}/{collection}/{id}/{}", relation.segment());
        let back_key = if relation == Relation::Subcomponents {
            "parent"
        } else {
            type_singular(kind)
        };
        let links = BTreeMap::from([
            ("self".to_owned(), self_link),
            (back_key.to_owned(), format!("{API_BASE}/{collection}/{id}")),
        ]);

        let total_count = items.len();
        Ok(Collection {
            items,
            links: Some(links),
            x_medkit: CollectionMeta { total_count },
        })
    }

    /// The global fault list (`GET /faults`), de-duplicated by fault code.
    #[must_use]
    pub fn global_faults(&self, filter: FaultStatusFilter) -> FaultList {
        let mut seen = std::collections::HashSet::new();
        let items: Vec<FaultSummary> = self
            .faults
            .values()
            .flatten()
            .filter(|f| filter.matches(&f.status))
            .filter(|f| seen.insert(f.fault_code.clone()))
            .cloned()
            .collect();
        FaultList {
            x_medkit: FaultListMeta {
                count: count(&items),
                muted_count: Some(0),
                cluster_count: Some(0),
                ..FaultListMeta::default()
            },
            items,
        }
    }

    /// An entity-scoped fault list (`…/{id}/faults`).
    ///
    /// A component aggregates over the apps it hosts and carries the
    /// `aggregation_*` extension keys; other kinds carry the app-scoped
    /// `entity_id` / `source_id` extension.
    ///
    /// # Errors
    ///
    /// [`ResolveError::NotFound`] if no entity of `kind` has id `id`.
    pub fn entity_faults(
        &self,
        kind: EntityKind,
        id: &str,
        filter: FaultStatusFilter,
    ) -> Result<FaultList, ResolveError> {
        let entity = self.require(kind, id)?;
        let items: Vec<FaultSummary> = self
            .faults
            .get(id)
            .into_iter()
            .flatten()
            .filter(|f| filter.matches(&f.status))
            .cloned()
            .collect();

        let x_medkit = if kind == EntityKind::Component {
            let sources = self.aggregation_sources(id);
            FaultListMeta {
                count: count(&items),
                entity_id: Some(id.to_owned()),
                aggregated: Some(true),
                aggregation_level: Some("component".to_owned()),
                app_count: Some(u32::try_from(sources.len()).unwrap_or(u32::MAX)),
                aggregation_sources: Some(sources),
                ..FaultListMeta::default()
            }
        } else {
            FaultListMeta {
                count: count(&items),
                muted_count: Some(0),
                cluster_count: Some(0),
                entity_id: Some(id.to_owned()),
                source_id: entity
                    .x_medkit
                    .as_ref()
                    .and_then(|m| m.ros2.as_ref())
                    .map(|r| r.node.clone()),
                ..FaultListMeta::default()
            }
        };

        Ok(FaultList { items, x_medkit })
    }

    /// The single fault detail (`…/{id}/faults/{fault_code}`).
    ///
    /// Derived best-effort from the fault summary: the DTC `item` with its
    /// camelCase status sub-object and the `x-medkit` extension are exact; the
    /// freeze-frame `snapshots` array is empty here, since rich freeze-frame
    /// capture arrives with the live binding slice (a documented gap).
    ///
    /// # Errors
    ///
    /// [`ResolveError::NotFound`] if the entity or the fault code is unknown.
    pub fn fault_detail(
        &self,
        kind: EntityKind,
        id: &str,
        fault_code: &str,
    ) -> Result<FaultDetail<Value>, ResolveError> {
        self.require(kind, id)?;
        let summary = self
            .faults
            .get(id)
            .into_iter()
            .flatten()
            .find(|f| f.fault_code == fault_code)
            .ok_or_else(|| ResolveError::fault_not_found(fault_code))?;
        Ok(detail_from_summary(summary))
    }

    /// Acknowledge a fault deletion (`DELETE …/{id}/faults/{fault_code}`).
    ///
    /// The read-only skeleton cannot mutate the snapshot, so this verifies the
    /// fault exists (so the call is `204`, not a silent no-op) without changing
    /// state. A write-through path lands with the binding slice.
    ///
    /// # Errors
    ///
    /// [`ResolveError::NotFound`] if the entity or the fault code is unknown.
    pub fn delete_fault(
        &self,
        kind: EntityKind,
        id: &str,
        fault_code: &str,
    ) -> Result<(), ResolveError> {
        self.fault_detail(kind, id, fault_code).map(|_| ())
    }

    /// Readable data under an entity (`…/{id}/data[/{topic_path}]`).
    ///
    /// With no `topic_path`, returns the entity's whole data tree (an empty
    /// object if it exposes none); a `topic_path` navigates into it by `/`.
    ///
    /// # Errors
    ///
    /// [`ResolveError::NotFound`] if the entity or the addressed topic path is
    /// unknown.
    pub fn data(
        &self,
        kind: EntityKind,
        id: &str,
        topic_path: Option<&str>,
    ) -> Result<Value, ResolveError> {
        self.require(kind, id)?;
        let root = self.data.get(id).cloned().unwrap_or_else(|| json!({}));
        match topic_path {
            None | Some("") => Ok(root),
            Some(path) => {
                let mut cursor = &root;
                for segment in path.split('/').filter(|s| !s.is_empty()) {
                    cursor = cursor
                        .get(segment)
                        .ok_or_else(|| ResolveError::data_not_found(path))?;
                }
                Ok(cursor.clone())
            }
        }
    }

    /// The ros2 node FQNs of the apps a component hosts, for fault aggregation.
    fn aggregation_sources(&self, component_id: &str) -> Vec<String> {
        self.relationships
            .iter()
            .filter(|edge| edge.from_id == component_id && edge.relation == Relation::Hosts)
            .filter_map(|edge| {
                edge.item
                    .x_medkit
                    .as_ref()
                    .and_then(|m| m.ros2.as_ref())
                    .map(|r| r.node.clone())
            })
            .collect()
    }

    /// The liveness document (`GET /health`).
    ///
    /// Best-effort: reports `status: healthy` plus the entity-cache counts the
    /// view actually knows. The rich `x-medkit-*` provider/executor telemetry
    /// blocks of the captured contract are server-rendered from internals this
    /// skeleton does not yet have (a documented gap).
    #[must_use]
    pub fn health_document(&self) -> Value {
        let count_kind = |kind: EntityKind| self.entities.iter().filter(|e| e.kind == kind).count();
        json!({
            "status": "healthy",
            "discovery": { "mode": "runtime_only", "strategy": "runtime" },
            "x-medkit-entity-cache": {
                "areas": count_kind(EntityKind::Area),
                "components": count_kind(EntityKind::Component),
                "apps": count_kind(EntityKind::App),
                "functions": count_kind(EntityKind::Function)
            }
        })
    }
}

/// The capability catalogue served at the API root (`GET /`).
///
/// Best-effort, contract-shaped: it advertises only the families this skeleton
/// actually serves (faults, data access, discovery); deferred families are
/// advertised as unavailable and answer `501`.
#[must_use]
pub fn root_document() -> Value {
    json!({
        "api_base": API_BASE,
        "capabilities": {
            "aggregation": false,
            "async_actions": false,
            "authentication": false,
            "bulk_data": false,
            "configurations": false,
            "cyclic_subscriptions": false,
            "data_access": true,
            "discovery": true,
            "faults": true,
            "locking": false,
            "logs": false,
            "operations": false,
            "scripts": false,
            "tls": false,
            "triggers": false,
            "updates": false,
            "vendor_extensions": false
        },
        "endpoints": endpoint_catalogue(),
        "name": "taktora-medkit Gateway",
        "version": env!("CARGO_PKG_VERSION")
    })
}

/// The endpoint catalogue advertised at the root: the read-core surface.
fn endpoint_catalogue() -> Vec<String> {
    let mut endpoints = vec![
        format!("GET {API_BASE}/"),
        format!("GET {API_BASE}/version-info"),
        format!("GET {API_BASE}/health"),
        format!("GET {API_BASE}/areas"),
        format!("GET {API_BASE}/components"),
        format!("GET {API_BASE}/apps"),
        format!("GET {API_BASE}/functions"),
        format!("GET {API_BASE}/faults"),
    ];
    for kind in [
        EntityKind::Area,
        EntityKind::Component,
        EntityKind::App,
        EntityKind::Function,
    ] {
        let collection = collection_segment(kind);
        endpoints.push(format!("GET {API_BASE}/{collection}/{{id}}"));
        endpoints.push(format!("GET {API_BASE}/{collection}/{{id}}/faults"));
        endpoints.push(format!(
            "GET {API_BASE}/{collection}/{{id}}/faults/{{fault_code}}"
        ));
        endpoints.push(format!("GET {API_BASE}/{collection}/{{id}}/data"));
    }
    endpoints
}

/// The version catalogue (`GET /version-info`).
#[must_use]
pub fn version_info_document() -> Value {
    json!({
        "items": [
            {
                "base_uri": API_BASE,
                "vendor_info": {
                    "name": "taktora-medkit",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "version": SOVD_VERSION
            }
        ]
    })
}

fn count(items: &[FaultSummary]) -> u32 {
    u32::try_from(items.len()).unwrap_or(u32::MAX)
}

fn bool01(value: bool) -> String {
    if value { "1" } else { "0" }.to_owned()
}

fn detail_from_summary(summary: &FaultSummary) -> FaultDetail<Value> {
    let confirmed = summary.status == "CONFIRMED";
    let pending = summary.status == "PREFAILED" || summary.status == "PENDING";
    FaultDetail {
        environment_data: EnvironmentData {
            extended_data_records: ExtendedDataRecords {
                first_occurrence: iso8601(summary.first_occurred),
                last_occurrence: iso8601(summary.last_occurred),
            },
            snapshots: Vec::new(),
        },
        item: FaultItem {
            code: summary.fault_code.clone(),
            fault_name: summary.description.clone(),
            severity: summary.severity,
            status: DtcStatus {
                aggregated_status: if confirmed { "active" } else { "pending" }.to_owned(),
                test_failed: bool01(confirmed || pending),
                confirmed_dtc: bool01(confirmed),
                pending_dtc: bool01(pending),
            },
        },
        x_medkit: FaultDetailMeta {
            occurrence_count: summary.occurrence_count,
            reporting_sources: summary.reporting_sources.clone(),
            severity_label: summary.severity_label.clone(),
            status_raw: summary.status.clone(),
        },
    }
}

/// Format fractional epoch seconds as an ISO-8601 UTC timestamp with
/// millisecond precision (e.g. `2026-06-28T15:45:00.750Z`).
///
/// Uses Howard Hinnant's `civil_from_days` algorithm so the gateway carries no
/// date-library dependency, holding the lean-core invariant (`REQ_0916`).
#[allow(
    clippy::cast_possible_truncation,
    reason = "fault epoch seconds are far inside i64 millis range; round() before cast"
)]
fn iso8601(epoch_seconds: f64) -> String {
    let total_millis = (epoch_seconds * 1000.0).round() as i64;
    let mut secs = total_millis.div_euclid(1000);
    let millis = total_millis.rem_euclid(1000);
    let days = secs.div_euclid(86_400);
    secs = secs.rem_euclid(86_400);
    let (hour, minute, second) = (secs / 3600, (secs % 3600) / 60, secs % 60);

    // civil_from_days: days since 1970-01-01 -> (year, month, day).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

#[cfg(test)]
mod tests {
    use super::*;
    use taktora_medkit_model::{EntityMeta, Ros2Ref, Severity};
    use taktora_medkit_provider::{MockProvider, Provider};

    fn app(id: &str) -> Entity {
        Entity {
            href: format!("{API_BASE}/apps/{id}"),
            id: id.to_owned(),
            name: id.to_owned(),
            kind: EntityKind::App,
            parent_id: None,
            description: None,
            x_medkit: Some(EntityMeta {
                is_online: Some(true),
                ros2: Some(Ros2Ref {
                    node: format!("/{id}"),
                }),
                source: Some("heuristic".to_owned()),
                ..EntityMeta::default()
            }),
        }
    }

    fn component(id: &str) -> Entity {
        Entity {
            href: format!("{API_BASE}/components/{id}"),
            id: id.to_owned(),
            name: id.to_owned(),
            kind: EntityKind::Component,
            parent_id: None,
            description: None,
            x_medkit: None,
        }
    }

    fn fault(code: &str, severity: Severity, status: &str) -> FaultSummary {
        FaultSummary {
            description: code.to_owned(),
            fault_code: code.to_owned(),
            first_occurred: 1_782_600_000.25,
            last_occurred: 1_782_661_500.75,
            occurrence_count: 7,
            reporting_sources: vec![format!("/{code}")],
            severity: severity.wire_value(),
            severity_label: format!("{severity:?}").to_uppercase(),
            status: status.to_owned(),
        }
    }

    fn view() -> MergedView {
        let provider = MockProvider::new()
            .with_entity(component("spark"))
            .with_entity(app("gw"))
            .with_relationship("spark", Relation::Hosts, app("gw"))
            .with_fault("gw", fault("BRAKE", Severity::Error, "CONFIRMED"))
            .with_fault("spark", fault("BRAKE", Severity::Error, "CONFIRMED"))
            .with_fault("spark", fault("MOTOR", Severity::Warn, "PREFAILED"))
            .with_data("spark", json!({ "cpu": { "load": 0.4 } }));
        MergedView::from_snapshot(provider.snapshot())
    }

    /// `TEST_0906` — top-level lists and relationship sub-resources resolve with
    /// the right envelope (no `_links` on lists; `self` + back-link on relations).
    #[test]
    fn lists_and_relationships_resolve() {
        let view = view();
        let apps = view.list(EntityKind::App);
        assert_eq!(apps.x_medkit.total_count, 1);
        assert!(apps.links.is_none());

        let hosts = view
            .relationship(EntityKind::Component, "spark", Relation::Hosts)
            .unwrap();
        assert_eq!(hosts.items.len(), 1);
        let links = hosts.links.unwrap();
        assert_eq!(links["self"], "/api/v1/components/spark/hosts");
        assert_eq!(links["component"], "/api/v1/components/spark");

        // Empty relationship still emits the envelope with links.
        let depends = view
            .relationship(EntityKind::Component, "spark", Relation::DependsOn)
            .unwrap();
        assert_eq!(depends.items.len(), 0);
        assert!(depends.links.is_some());
    }

    /// `TEST_0906` — subcomponents back-links via the `parent` key, not `component`.
    #[test]
    fn subcomponents_links_via_parent() {
        let links = view()
            .relationship(EntityKind::Component, "spark", Relation::Subcomponents)
            .unwrap()
            .links
            .unwrap();
        assert!(links.contains_key("parent"));
        assert!(!links.contains_key("component"));
    }

    /// `TEST_0906` — an unknown entity id is a not-found error, not a panic.
    #[test]
    fn unknown_entity_is_not_found() {
        let err = view()
            .relationship(EntityKind::App, "nope", Relation::BelongsTo)
            .unwrap_err();
        let ResolveError::NotFound(error) = err;
        assert_eq!(error.error_code, "entity-not-found");
    }

    /// `TEST_0906` — fault filtering and the per-scope `x-medkit` extension keys.
    #[test]
    fn fault_scopes_and_filters() {
        let view = view();
        // Global de-dups by fault code across entities.
        assert_eq!(view.global_faults(FaultStatusFilter::All).items.len(), 2);
        assert_eq!(
            view.global_faults(FaultStatusFilter::Pending).items.len(),
            1
        );

        let comp = view
            .entity_faults(EntityKind::Component, "spark", FaultStatusFilter::All)
            .unwrap();
        assert_eq!(comp.x_medkit.aggregated, Some(true));
        assert_eq!(
            comp.x_medkit.aggregation_level.as_deref(),
            Some("component")
        );

        let app = view
            .entity_faults(EntityKind::App, "gw", FaultStatusFilter::All)
            .unwrap();
        assert_eq!(app.x_medkit.source_id.as_deref(), Some("/gw"));
        assert!(app.x_medkit.aggregated.is_none());
    }

    /// `TEST_0906` — fault detail derives the camelCase DTC status sub-object.
    #[test]
    fn fault_detail_derives_status() {
        let detail = view().fault_detail(EntityKind::App, "gw", "BRAKE").unwrap();
        assert_eq!(detail.item.code, "BRAKE");
        assert_eq!(detail.item.status.confirmed_dtc, "1");
        assert_eq!(detail.item.status.aggregated_status, "active");
        assert!(view().fault_detail(EntityKind::App, "gw", "NOPE").is_err());
    }

    /// `TEST_0906` — data navigates the topic path and 404s past the leaf.
    #[test]
    fn data_navigates_topic_path() {
        let view = view();
        assert_eq!(
            view.data(EntityKind::Component, "spark", Some("cpu/load"))
                .unwrap(),
            json!(0.4)
        );
        assert!(
            view.data(EntityKind::Component, "spark", Some("cpu/missing"))
                .is_err()
        );
        // Unknown entity has no data tree.
        assert!(view.data(EntityKind::App, "gw", None).unwrap().is_object());
    }

    /// `TEST_0906` — the ISO-8601 formatter matches the contract timestamp shape.
    #[test]
    fn iso8601_formats_epoch() {
        assert_eq!(iso8601(1_782_661_500.75), "2026-06-28T15:45:00.750Z");
        assert_eq!(iso8601(0.0), "1970-01-01T00:00:00.000Z");
    }
}
