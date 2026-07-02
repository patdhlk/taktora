//! The merged read-model and the pure read-family resolvers over it.
//!
//! [`MergePipeline`] folds one or more [`ProviderSnapshot`]s (and, in a later
//! slice, a manifest) into a single [`MergedView`]; the resolver methods on
//! [`MergedView`] are pure `(&MergedView, params) -> wire-type` functions the
//! HTTP layer calls. Keeping them pure and transport-neutral lets the same
//! resolvers run over a mock, a manifest, or live taktora bindings (`REQ_0916`).

use std::collections::{BTreeMap, HashMap, HashSet};

use serde_json::{Value, json};
use taktora_medkit_manifest::Manifest;
use taktora_medkit_model::Entity;
use taktora_medkit_model::{
    BuildInfo, Collection, CollectionMeta, DtcStatus, EntityKind, EnvironmentData,
    ExtendedDataRecords, FaultDetail, FaultDetailMeta, FaultItem, FaultList, FaultListMeta,
    FaultSummary, GenericError,
};
use taktora_medkit_provider::{LogEntry, ProviderSnapshot, Relation, RelationshipEdge, Telemetry};

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

/// Folds provider snapshots and the grouping manifest into one [`MergedView`].
///
/// The walking skeleton merges a single mock snapshot — an identity fold — but
/// the seam is shaped for the downstream slices: #82 applies the manifest here
/// (via [`MergePipeline::with_manifest`]), and #83/#84 contribute additional
/// [`ProviderSnapshot`]s to be merged.
#[derive(Clone, Debug, Default)]
pub struct MergePipeline {
    snapshots: Vec<ProviderSnapshot>,
    manifest: Option<Manifest>,
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

    /// Apply a grouping [`Manifest`] when folding: its declared Areas/Components
    /// become entities and the binding-emitted raw entities (`app:<task>`,
    /// `component:<subdevice>`) are re-parented under them (`REQ_0921`).
    ///
    /// An empty manifest is a no-op, leaving the flat provider grouping intact
    /// (`REQ_0922`).
    #[must_use]
    pub fn with_manifest(mut self, manifest: Manifest) -> Self {
        self.manifest = Some(manifest);
        self
    }

    /// Fold the accumulated snapshots (and the manifest, if any) into the merged
    /// read-model.
    #[must_use]
    pub fn merge(self) -> MergedView {
        let mut entities: Vec<Entity> = Vec::new();
        let mut by_id: HashMap<String, usize> = HashMap::new();
        let mut relationships: Vec<RelationshipEdge> = Vec::new();
        let mut faults: BTreeMap<String, Vec<FaultSummary>> = BTreeMap::new();
        let mut fault_environments: BTreeMap<String, BTreeMap<String, EnvironmentData<Value>>> =
            BTreeMap::new();
        let mut data: BTreeMap<String, Value> = BTreeMap::new();
        let mut logs: BTreeMap<String, Vec<LogEntry>> = BTreeMap::new();
        let mut telemetry = Telemetry::default();

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
            for (entity_id, envs) in snapshot.fault_environments {
                fault_environments
                    .entry(entity_id)
                    .or_default()
                    .extend(envs);
            }
            data.extend(snapshot.data);
            logs.extend(snapshot.logs);
            // Fold provider telemetry, later snapshot wins per override key
            // (`REQ_0978`), like `data` above.
            telemetry.extend(snapshot.telemetry);
        }

        if let Some(manifest) = self.manifest.filter(|m| !m.is_empty()) {
            apply_manifest(&mut entities, &mut by_id, &mut relationships, &manifest);
        }

        MergedView {
            entities,
            by_id,
            relationships,
            faults,
            fault_environments,
            data,
            logs,
            telemetry,
        }
    }
}

/// Apply the grouping manifest to the folded read-model in place: inject the
/// declared skeleton, re-parent the raw provider entities, and synthesize the
/// relationship edges that surface the declared structure under the relationship
/// sub-resources (`/areas/{id}/components`, `/components/{id}/hosts`,
/// `/components/{id}/subcomponents`) — `REQ_0921`.
fn apply_manifest(
    entities: &mut Vec<Entity>,
    by_id: &mut HashMap<String, usize>,
    relationships: &mut Vec<RelationshipEdge>,
    manifest: &Manifest,
) {
    // 1. Upsert the declared Area/Component skeleton (declared wins on id).
    for declared in manifest.declared_entities() {
        if let Some(&idx) = by_id.get(&declared.id) {
            entities[idx] = declared;
        } else {
            by_id.insert(declared.id.clone(), entities.len());
            entities.push(declared);
        }
    }

    // 2. Re-parent the binding-emitted raw entities under their declared parent.
    for entity in entities.iter_mut() {
        if let Some(parent) = manifest.parent_of(&entity.id) {
            entity.parent_id = Some(parent.to_owned());
        }
    }

    // 3. Synthesize relationship edges from the (now re-parented) hierarchy,
    //    skipping any an upstream snapshot already supplied.
    let kind_of: HashMap<String, EntityKind> =
        entities.iter().map(|e| (e.id.clone(), e.kind)).collect();
    let mut seen: HashSet<(String, Relation, String)> = relationships
        .iter()
        .map(|e| (e.from_id.clone(), e.relation, e.item.id.clone()))
        .collect();

    for child in entities.clone() {
        let Some(parent_id) = child.parent_id.as_deref() else {
            continue;
        };
        let Some(&parent_kind) = kind_of.get(parent_id) else {
            continue;
        };
        for &relation in synthesized_relations(parent_kind, child.kind) {
            let key = (parent_id.to_owned(), relation, child.id.clone());
            if seen.insert(key) {
                relationships.push(RelationshipEdge {
                    from_id: parent_id.to_owned(),
                    relation,
                    item: rel_item(&child),
                });
            }
        }
    }
}

/// The relationship edges a `parent` → `child` hierarchy edge surfaces as, by the
/// kinds at each end. An Area groups its components (and `contains` them); a
/// Component hosts its apps and nests its subcomponents.
const fn synthesized_relations(parent: EntityKind, child: EntityKind) -> &'static [Relation] {
    match (parent, child) {
        (EntityKind::Area, EntityKind::Component) => &[Relation::Components, Relation::Contains],
        (EntityKind::Area, _) => &[Relation::Contains],
        (EntityKind::Component | EntityKind::Function, EntityKind::App) => &[Relation::Hosts],
        (EntityKind::Component, EntityKind::Component) => &[Relation::Subcomponents],
        _ => &[],
    }
}

/// Shape an entity as a relationship sub-resource item: a bare reference with no
/// inline parent and no top-level `component_id` decoration (those belong only on
/// the top-level `/apps` list item).
fn rel_item(entity: &Entity) -> Entity {
    let mut item = entity.clone();
    item.parent_id = None;
    if let Some(meta) = item.x_medkit.as_mut() {
        meta.component_id = None;
    }
    item
}

/// A consistent, indexed read-model the resolvers serve from.
#[derive(Clone, Debug, Default)]
pub struct MergedView {
    entities: Vec<Entity>,
    by_id: HashMap<String, usize>,
    relationships: Vec<RelationshipEdge>,
    faults: BTreeMap<String, Vec<FaultSummary>>,
    fault_environments: BTreeMap<String, BTreeMap<String, EnvironmentData<Value>>>,
    data: BTreeMap<String, Value>,
    logs: BTreeMap<String, Vec<LogEntry>>,
    telemetry: Telemetry,
}

impl MergedView {
    /// Build a view from a single snapshot (the common skeleton case).
    #[must_use]
    pub fn from_snapshot(snapshot: ProviderSnapshot) -> Self {
        MergePipeline::new().with_snapshot(snapshot).merge()
    }

    /// Build a view from a single snapshot, applying a grouping `manifest` when
    /// one is given (`REQ_0921`). A `None` or empty manifest yields the same flat
    /// grouping as [`MergedView::from_snapshot`] (`REQ_0922`).
    #[must_use]
    pub fn from_snapshot_with_manifest(
        snapshot: ProviderSnapshot,
        manifest: Option<Manifest>,
    ) -> Self {
        let mut pipeline = MergePipeline::new().with_snapshot(snapshot);
        if let Some(manifest) = manifest {
            pipeline = pipeline.with_manifest(manifest);
        }
        pipeline.merge()
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
    /// The DTC `item` with its camelCase status sub-object and the `x-medkit`
    /// extension are derived from the fault summary. When the merged snapshot
    /// carried freeze-frame environment data for `(id, fault_code)` (a capturing
    /// binding populated [`ProviderSnapshot::fault_environments`], `ADR_0116`,
    /// `REQ_0929`), the real `snapshots` / `extended_data_records` are surfaced;
    /// otherwise the occurrence-only environment shape is emitted (back-compat).
    ///
    /// [`ProviderSnapshot::fault_environments`]: taktora_medkit_provider::ProviderSnapshot::fault_environments
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
        let mut detail = detail_from_summary(summary);
        if let Some(env) = self
            .fault_environments
            .get(id)
            .and_then(|m| m.get(fault_code))
        {
            detail.environment_data = env.clone();
        }
        Ok(detail)
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

    /// Diagnostic log entries under an entity (`…/{id}/logs`), filtered by an
    /// optional exact `severity` match and an optional `context` substring match
    /// (`REQ_0976`).
    ///
    /// Lenient like [`data`](Self::data) is about an absent data tree: an entity
    /// with no log entries — known or unknown — yields an empty list rather than a
    /// `404`, so a polling client distinguishes "no logs" from "bad path" by the
    /// status code, not by guessing. Pure and clock-free.
    ///
    /// # Errors
    ///
    /// Returns [`ResolveError`] for symmetry with the other resolvers; the current
    /// lenient implementation never produces one.
    pub fn logs(
        &self,
        _kind: EntityKind,
        id: &str,
        severity: Option<&str>,
        context: Option<&str>,
    ) -> Result<Vec<LogEntry>, ResolveError> {
        let items = self
            .logs
            .get(id)
            .into_iter()
            .flatten()
            .filter(|entry| severity.is_none_or(|want| entry.severity == want))
            .filter(|entry| context.is_none_or(|want| entry.context.contains(want)))
            .cloned()
            .collect();
        Ok(items)
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

    /// The liveness document (`GET /health`), shaped to the captured golden
    /// (`contract/golden/health.json`) so a path/field-hardcoding client parses
    /// it unchanged (`REQ_0967`).
    ///
    /// The `x-medkit-entity-cache` counts are real (the view knows them). The
    /// `x-medkit-data-provider` and `x-medkit-subscription-executor` blocks start
    /// from a **best-effort placeholder** baseline — every counter a benign zero
    /// (`worker_alive`/`degraded` reflect the "nothing wrong" state) — over which
    /// any provider-sourced [`Telemetry`] is overlaid (`REQ_0978`): a provider
    /// that reports real pool/executor internals fills these in, while a provider
    /// without telemetry yields exactly the zero-filled blocks as before. The
    /// blocks stay present and field-complete either way, so a client never hits a
    /// missing key. The live entity-cache counts stay authoritative — an
    /// `entity_cache` override may add `generation`/`grew`/etc. but never
    /// overrides the four computed counts. The wall-clock `timestamp` is injected
    /// at the HTTP edge (this resolver stays clock-free and snapshot-testable).
    ///
    /// [`Telemetry`]: taktora_medkit_provider::Telemetry
    #[must_use]
    pub fn health_document(&self) -> Value {
        let count_kind = |kind: EntityKind| self.entities.iter().filter(|e| e.kind == kind).count();

        let mut entity_cache = json!({
            "capacity": 256,
            "generation": 0,
            "grew": false
        });
        overlay(&mut entity_cache, &self.telemetry.entity_cache);
        // The four live counts are authoritative — set them last so no telemetry
        // override can shadow them (`REQ_0978`).
        if let Some(obj) = entity_cache.as_object_mut() {
            obj.insert("areas".to_owned(), json!(count_kind(EntityKind::Area)));
            obj.insert(
                "components".to_owned(),
                json!(count_kind(EntityKind::Component)),
            );
            obj.insert("apps".to_owned(), json!(count_kind(EntityKind::App)));
            obj.insert(
                "functions".to_owned(),
                json!(count_kind(EntityKind::Function)),
            );
        }

        let mut data_provider = json!({
            "cold_wait_cap": 0,
            "concurrent_cold_waits": 0,
            "evictions_total": 0,
            "graph_events_received": 0,
            "pool_cap": 0,
            "pool_hits": 0,
            "pool_misses": 0,
            "pool_size": 0,
            "type_change_events": 0,
            "unsupported_type_count": 0
        });
        overlay(&mut data_provider, &self.telemetry.data_provider);

        let mut subscription_executor = json!({
            "current_task_age_ms": 0,
            "degraded": false,
            "graph_events_received": 0,
            "last_task_latency_us": 0,
            "max_task_latency_us": 0,
            "queue_depth": 0,
            "queue_dropped": 0,
            "queue_max_depth_observed": 0,
            "tasks_completed": 0,
            "tasks_failed": 0,
            "watchdog_trips": 0,
            "worker_alive": true
        });
        overlay(
            &mut subscription_executor,
            &self.telemetry.subscription_executor,
        );

        json!({
            "status": "healthy",
            "discovery": { "mode": "runtime_only", "strategy": "runtime" },
            "x-medkit-entity-cache": entity_cache,
            "x-medkit-data-provider": data_provider,
            "x-medkit-subscription-executor": subscription_executor
        })
    }
}

/// Overlay provider override `overrides` onto a default block `base` in place:
/// insert/overwrite each key (`REQ_0978`). A no-op when `overrides` is empty,
/// preserving the back-compat baseline.
fn overlay(base: &mut Value, overrides: &BTreeMap<String, Value>) {
    if let Some(obj) = base.as_object_mut() {
        for (key, value) in overrides {
            obj.insert(key.clone(), value.clone());
        }
    }
}

/// The capability catalogue served at the API root (`GET /`).
///
/// Contract-shaped and **honest** (`REQ_0965`): a flag is `true` exactly when the
/// gateway actually mounts that family's routes, so a capability-gating client
/// never skips a working endpoint nor probes a deferred one. The families this
/// skeleton serves — discovery, data access, faults, plus the vendor extensions
/// `authentication` (token endpoints), `locking` (diagnostic-scoped locks), and
/// `triggers` (CRUD + SSE) — are `true`; every family that still answers `501` is
/// `false`. `vendor_extensions` is `true` because locks/triggers are taktora
/// `x-medkit` extensions over the SOVD core.
#[must_use]
pub fn root_document() -> Value {
    json!({
        "api_base": API_BASE,
        "capabilities": {
            "aggregation": false,
            "async_actions": false,
            "authentication": true,
            "bulk_data": true,
            "configurations": true,
            "cyclic_subscriptions": true,
            "data_access": true,
            "discovery": true,
            "faults": true,
            "locking": true,
            "logs": true,
            "operations": true,
            "scripts": true,
            "tls": false,
            "triggers": true,
            "updates": true,
            "vendor_extensions": true
        },
        "endpoints": endpoint_catalogue(),
        "name": "taktora-medkit Gateway",
        "version": env!("CARGO_PKG_VERSION")
    })
}

/// The endpoint catalogue advertised at the root: the read-core surface plus the
/// served vendor extensions (`REQ_0965`). Every entry here is a route the gateway
/// actually mounts; deferred families are omitted (they answer `501`).
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
        format!("DELETE {API_BASE}/faults"),
        format!("GET {API_BASE}/faults/stream"),
        format!("GET {API_BASE}/triggers"),
        format!("POST {API_BASE}/triggers"),
        format!("GET {API_BASE}/triggers/events"),
        // Updates are a global family (`REQ_0974`): mounted at the top level, not
        // per entity, like `/faults` and `/triggers`.
        format!("GET {API_BASE}/updates"),
        format!("POST {API_BASE}/updates"),
        format!("POST {API_BASE}/auth/token"),
        format!("POST {API_BASE}/auth/authorize"),
        format!("POST {API_BASE}/auth/revoke"),
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
        // Entity-scoped triggers are exposed on every kind (`REQ_0962`).
        endpoints.push(format!("GET {API_BASE}/{collection}/{{id}}/triggers"));
        endpoints.push(format!("POST {API_BASE}/{collection}/{{id}}/triggers"));
        // Operations + async executions are exposed on every kind (`REQ_0969`).
        endpoints.push(format!("GET {API_BASE}/{collection}/{{id}}/operations"));
        endpoints.push(format!(
            "POST {API_BASE}/{collection}/{{id}}/operations/{{op}}/executions"
        ));
        // Configurations are exposed on every kind (`REQ_0971`).
        endpoints.push(format!("GET {API_BASE}/{collection}/{{id}}/configurations"));
        endpoints.push(format!(
            "PUT {API_BASE}/{collection}/{{id}}/configurations/{{config_id}}"
        ));
        // Logs (read entries + a config GET/PUT) are exposed on every kind
        // (`REQ_0976`).
        endpoints.push(format!("GET {API_BASE}/{collection}/{{id}}/logs"));
    }
    // Cyclic subscriptions are exposed on apps, components, and functions only
    // (`REQ_0977`).
    for kind in [EntityKind::Component, EntityKind::App, EntityKind::Function] {
        let collection = collection_segment(kind);
        endpoints.push(format!(
            "GET {API_BASE}/{collection}/{{id}}/cyclic-subscriptions"
        ));
        endpoints.push(format!(
            "POST {API_BASE}/{collection}/{{id}}/cyclic-subscriptions"
        ));
    }
    // Diagnostic-scoped locks are exposed on apps and components only (`REQ_0963`).
    for kind in [EntityKind::App, EntityKind::Component] {
        let collection = collection_segment(kind);
        endpoints.push(format!("GET {API_BASE}/{collection}/{{id}}/locks"));
        endpoints.push(format!("POST {API_BASE}/{collection}/{{id}}/locks"));
    }
    // Writable bulk-data is exposed on apps and components only (`REQ_0972`).
    for kind in [EntityKind::App, EntityKind::Component] {
        let collection = collection_segment(kind);
        endpoints.push(format!("GET {API_BASE}/{collection}/{{id}}/bulk-data"));
        endpoints.push(format!(
            "POST {API_BASE}/{collection}/{{id}}/bulk-data/{{category_id}}"
        ));
    }
    // Scripts (storage + executions) are exposed on apps and components only
    // (`REQ_0973`).
    for kind in [EntityKind::App, EntityKind::Component] {
        let collection = collection_segment(kind);
        endpoints.push(format!("GET {API_BASE}/{collection}/{{id}}/scripts"));
        endpoints.push(format!("POST {API_BASE}/{collection}/{{id}}/scripts"));
    }
    // Lifecycle-status (start/restart/shutdown transitions) is exposed on apps
    // and components only (`REQ_0975`).
    for kind in [EntityKind::App, EntityKind::Component] {
        let collection = collection_segment(kind);
        endpoints.push(format!("GET {API_BASE}/{collection}/{{id}}/status"));
        endpoints.push(format!("PUT {API_BASE}/{collection}/{{id}}/status/start"));
    }
    endpoints
}

/// The version catalogue (`GET /version-info`).
///
/// `build` is the injected source identity of the running binary (`REQ_0980`),
/// rendered additively under `vendor_info` alongside the existing crate
/// `version`, so a client written against the `ros2_medkit` contract reads the
/// document unchanged (`REQ_0911`). A binary that injects nothing passes
/// [`BuildInfo::default`], which reports `"unknown"`.
#[must_use]
pub fn version_info_document(build: &BuildInfo) -> Value {
    json!({
        "items": [
            {
                "base_uri": API_BASE,
                "vendor_info": {
                    "name": "taktora-medkit",
                    "version": env!("CARGO_PKG_VERSION"),
                    "git_sha": build.git_sha,
                    "git_short": build.git_short,
                    "git_describe": build.git_describe,
                    "git_dirty": build.git_dirty,
                    "build_timestamp": build.build_timestamp,
                    "rustc_version": build.rustc_version
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

    /// `REQ_0965` — the root capabilities are honest: served families are `true`,
    /// deferred ones `false`, and the catalogue lists the served extensions.
    #[test]
    fn root_capabilities_match_served_surface() {
        let root = root_document();
        let caps = &root["capabilities"];
        for served in [
            "data_access",
            "discovery",
            "faults",
            "authentication",
            "locking",
            "triggers",
            "operations",
            "configurations",
            "bulk_data",
            "scripts",
            "updates",
            "logs",
            "cyclic_subscriptions",
            "vendor_extensions",
        ] {
            assert_eq!(
                caps[served], true,
                "{served} is served, must advertise true"
            );
        }
        let endpoints: Vec<&str> = root["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert!(endpoints.contains(&"GET /api/v1/faults/stream"));
        assert!(endpoints.contains(&"DELETE /api/v1/faults"));
    }

    /// `REQ_0967` — the health document carries the golden's `x-medkit-*`
    /// telemetry blocks, field-complete, alongside the real entity-cache counts.
    #[test]
    fn health_document_has_golden_telemetry_blocks() {
        let view = MergedView::from_snapshot(MockProvider::new().with_entity(app("gw")).snapshot());
        let health = view.health_document();
        assert_eq!(health["status"], "healthy");
        assert_eq!(health["x-medkit-entity-cache"]["apps"], 1);
        assert!(health["x-medkit-entity-cache"]["capacity"].is_number());
        assert!(health["x-medkit-data-provider"]["pool_cap"].is_number());
        assert_eq!(
            health["x-medkit-subscription-executor"]["worker_alive"],
            true
        );
    }

    /// `REQ_0978` — provider-sourced telemetry overlays the `x-medkit-*` blocks:
    /// supplied override keys surface as real values, while the live entity-cache
    /// counts stay authoritative and un-overridable.
    #[test]
    fn health_document_overlays_provider_telemetry() {
        let telemetry = Telemetry {
            data_provider: BTreeMap::from([("pool_cap".to_owned(), json!(256))]),
            subscription_executor: BTreeMap::from([("worker_alive".to_owned(), json!(false))]),
            // generation is overlaid; an `apps` override must NOT win over the count.
            entity_cache: BTreeMap::from([
                ("generation".to_owned(), json!(7)),
                ("apps".to_owned(), json!(999)),
            ]),
        };
        let provider = MockProvider::new()
            .with_entity(app("gw"))
            .with_telemetry(telemetry);
        let health = MergedView::from_snapshot(provider.snapshot()).health_document();

        assert_eq!(health["x-medkit-data-provider"]["pool_cap"], 256);
        assert_eq!(
            health["x-medkit-subscription-executor"]["worker_alive"],
            false
        );
        assert_eq!(health["x-medkit-entity-cache"]["generation"], 7);
        // The live count wins over the override.
        assert_eq!(health["x-medkit-entity-cache"]["apps"], 1);
    }

    /// `REQ_0978` — a provider with no telemetry yields exactly today's zero-filled
    /// blocks (back-compat), with the real counts intact.
    #[test]
    fn health_document_without_telemetry_keeps_zero_baseline() {
        let view = MergedView::from_snapshot(MockProvider::new().with_entity(app("gw")).snapshot());
        let health = view.health_document();
        assert_eq!(health["x-medkit-data-provider"]["pool_cap"], 0);
        assert_eq!(health["x-medkit-entity-cache"]["generation"], 0);
        assert_eq!(health["x-medkit-entity-cache"]["apps"], 1);
    }

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

    /// `TEST_0918` — a fault carrying environment data in the snapshot surfaces a
    /// non-empty freeze-frame under `fault_detail`; one without falls back to the
    /// occurrence-only shape (back-compat) — `REQ_0929`.
    #[test]
    fn fault_detail_surfaces_freeze_frame_environment() {
        use taktora_medkit_model::{FreezeFrame, FreezeFrameMeta};

        let payload = json!({ "wkc": 2, "expected_wkc": 3 });
        let env = EnvironmentData {
            extended_data_records: ExtendedDataRecords {
                first_occurrence: "2026-06-27T22:40:00.250Z".to_owned(),
                last_occurrence: "2026-06-28T15:45:00.750Z".to_owned(),
            },
            snapshots: vec![FreezeFrame {
                data: payload.clone(),
                name: "freeze_frame_at_confirmation".to_owned(),
                kind: "freeze_frame".to_owned(),
                x_medkit: FreezeFrameMeta {
                    captured_at: "2026-06-28T15:45:00.750Z".to_owned(),
                    full_data: payload,
                    message_type: "diagnostic_msgs/msg/DiagnosticStatus".to_owned(),
                    topic: "/diagnostics/brake_state".to_owned(),
                },
            }],
        };

        let provider = MockProvider::new()
            .with_entity(component("spark"))
            .with_fault("spark", fault("BRAKE", Severity::Error, "CONFIRMED"))
            .with_fault("spark", fault("MOTOR", Severity::Warn, "PREFAILED"))
            .with_fault_environment("spark", "BRAKE", env);
        let view = MergedView::from_snapshot(provider.snapshot());

        // The fault WITH environment data surfaces the real freeze-frame.
        let detail = view
            .fault_detail(EntityKind::Component, "spark", "BRAKE")
            .unwrap();
        assert_eq!(detail.environment_data.snapshots.len(), 1);
        assert_eq!(detail.environment_data.snapshots[0].kind, "freeze_frame");
        assert_eq!(
            detail.environment_data.snapshots[0].x_medkit.full_data["wkc"],
            json!(2)
        );
        assert_eq!(
            detail
                .environment_data
                .extended_data_records
                .first_occurrence,
            "2026-06-27T22:40:00.250Z"
        );
        // The DTC `item` still derives from the summary (faults_list contract).
        assert_eq!(detail.item.code, "BRAKE");

        // The fault WITHOUT environment data falls back to the empty shape.
        let plain = view
            .fault_detail(EntityKind::Component, "spark", "MOTOR")
            .unwrap();
        assert!(plain.environment_data.snapshots.is_empty());
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

    fn raw_app(task: &str) -> Entity {
        Entity {
            href: format!("{API_BASE}/apps/app:{task}"),
            id: format!("app:{task}"),
            name: task.to_owned(),
            kind: EntityKind::App,
            parent_id: None,
            description: None,
            x_medkit: Some(EntityMeta {
                component_id: Some("stale".to_owned()),
                is_online: Some(true),
                ros2: Some(Ros2Ref {
                    node: format!("/{task}"),
                }),
                source: Some("heuristic".to_owned()),
            }),
        }
    }

    fn raw_subdevice(addr: &str) -> Entity {
        Entity {
            href: format!("{API_BASE}/components/component:{addr}"),
            id: format!("component:{addr}"),
            name: addr.to_owned(),
            kind: EntityKind::Component,
            parent_id: None,
            description: None,
            x_medkit: None,
        }
    }

    fn manifest() -> taktora_medkit_manifest::Manifest {
        taktora_medkit_manifest::Manifest::builder()
            .area("drive", "Drive train")
            .component("nav", "drive", "Navigation")
            .map_task("planner", "nav")
            .map_subdevice("0x01", "nav")
            .build()
    }

    /// `TEST_0910` — applying a manifest injects the declared skeleton, re-parents
    /// the raw entities, and surfaces the declared structure under the
    /// relationship sub-resources (`REQ_0920`, `REQ_0921`).
    #[test]
    fn manifest_reparents_into_declared_structure() {
        let snapshot = MockProvider::new()
            .with_entity(raw_app("planner"))
            .with_entity(raw_subdevice("0x01"))
            .snapshot();
        let view = MergePipeline::new()
            .with_snapshot(snapshot)
            .with_manifest(manifest())
            .merge();

        // Declared Area + Component became entities.
        assert_eq!(view.entity("drive").unwrap().kind, EntityKind::Area);
        assert_eq!(view.entity("nav").unwrap().kind, EntityKind::Component);

        // Area groups its declared component under /components and /contains.
        let components = view
            .relationship(EntityKind::Area, "drive", Relation::Components)
            .unwrap();
        assert_eq!(components.items.len(), 1);
        assert_eq!(components.items[0].id, "nav");

        // Component hosts the re-parented app (component_id stripped on the item).
        let hosts = view
            .relationship(EntityKind::Component, "nav", Relation::Hosts)
            .unwrap();
        assert_eq!(hosts.items.len(), 1);
        assert_eq!(hosts.items[0].id, "app:planner");
        assert!(
            hosts.items[0]
                .x_medkit
                .as_ref()
                .unwrap()
                .component_id
                .is_none()
        );

        // Component nests the re-parented subdevice as a subcomponent.
        let subs = view
            .relationship(EntityKind::Component, "nav", Relation::Subcomponents)
            .unwrap();
        assert_eq!(subs.items.len(), 1);
        assert_eq!(subs.items[0].id, "component:0x01");
    }

    /// `TEST_0911` — an empty (or absent) manifest leaves the flat provider
    /// grouping untouched, with no panic (`REQ_0922`).
    #[test]
    fn empty_manifest_falls_back_to_flat() {
        let snapshot = MockProvider::new()
            .with_entity(raw_app("planner"))
            .snapshot();
        let view = MergePipeline::new()
            .with_snapshot(snapshot)
            .with_manifest(taktora_medkit_manifest::Manifest::default())
            .merge();

        // No declared skeleton injected; the raw app keeps no parent.
        assert!(view.entity("drive").is_none());
        assert!(view.entity("app:planner").unwrap().parent_id.is_none());
        // The parentless app surfaces under no synthesized relationship.
        assert_eq!(view.list(EntityKind::App).items.len(), 1);
        assert!(view.list(EntityKind::Area).items.is_empty());
    }

    /// `REQ_0980` — the version catalogue carries the injected build identity
    /// under `vendor_info`, additively and with the specified types.
    #[test]
    fn version_info_renders_injected_build_identity() {
        let build = BuildInfo {
            git_sha: "d74603ddeadbeef".to_owned(),
            git_short: "d74603d".to_owned(),
            git_describe: "v0.3.0-2-gd74603d".to_owned(),
            git_dirty: true,
            build_timestamp: "2026-07-03T09:15:00Z".to_owned(),
            rustc_version: "rustc 1.86.0".to_owned(),
        };
        let doc = version_info_document(&build);
        let vendor = &doc["items"][0]["vendor_info"];

        // Existing fields untouched (drop-in compat, `REQ_0911`).
        assert_eq!(vendor["name"], "taktora-medkit");
        assert_eq!(doc["items"][0]["version"], SOVD_VERSION);
        // Build identity, typed: strings for the git/timestamp/rustc fields, a
        // JSON boolean for the dirty flag.
        assert_eq!(vendor["git_sha"], "d74603ddeadbeef");
        assert_eq!(vendor["git_short"], "d74603d");
        assert_eq!(vendor["git_describe"], "v0.3.0-2-gd74603d");
        assert_eq!(vendor["build_timestamp"], "2026-07-03T09:15:00Z");
        assert_eq!(vendor["rustc_version"], "rustc 1.86.0");
        assert_eq!(vendor["git_dirty"], serde_json::Value::Bool(true));
    }

    /// `REQ_0980` — with no injected identity the document is still well-formed:
    /// the git fields report `"unknown"` and the tree reads clean.
    #[test]
    fn version_info_defaults_to_unknown() {
        let doc = version_info_document(&BuildInfo::default());
        let vendor = &doc["items"][0]["vendor_info"];
        assert_eq!(vendor["git_sha"], "unknown");
        assert_eq!(vendor["build_timestamp"], "unknown");
        assert_eq!(vendor["git_dirty"], serde_json::Value::Bool(false));
    }
}
