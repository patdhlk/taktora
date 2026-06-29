//! Triggers + the SSE fault-event stream (`REQ_0930`–`REQ_0934`, issue #85).
//!
//! Two cooperating pieces sit off the request path on the tokio side:
//!
//! - A **refresh-and-diff loop** ([`refresh_loop`]) that re-polls the provider on
//!   a cadence, folds each snapshot into a [`MergedView`], diffs it against the
//!   previous one, and broadcasts the change events. The served read-model is
//!   hot-swapped through a [`watch`] channel so the read-core stays live too.
//! - A **trigger registry** ([`TriggerStore`]) behind the `/api/v1/triggers` CRUD
//!   surface, and the [`events_stream`] SSE endpoint that filters the broadcast by
//!   the registered triggers.
//!
//! # Contract reconciliation: our event vocabulary, the golden's frame shape
//!
//! The captured golden (`contract/golden/faults_stream_sse_sample.txt` /
//! `faults_stream_event.json`) frames each event as
//! `id: <n>\nevent: <event_type>\ndata: <json>\n\n`, where the `data` object
//! carries `event_type`, a full `fault` sub-object, a `timestamp`, and an
//! `x-medkit` (`entity_id`, `entity_type`). The captured `event_type` value is
//! `fault_confirmed`.
//!
//! taktora's events are **diff-derived**, so the vocabulary is ours —
//! `fault_raised`, `fault_cleared`, `health_changed` — but the **frame and
//! data-object shape are the golden's**, byte-for-byte, so a drop-in
//! `ros2_medkit` client parses our stream unchanged. A `health_changed` event
//! carries the fault that drove the transition (the worst current fault, or the
//! just-cleared fault when health returns to OK) so the uniform golden shape
//! holds. We deliberately do **not** emit `fault_confirmed`; the frame envelope,
//! not the vocabulary, is the compatibility contract.
//!
//! # Deferred: rich trigger conditions (#87)
//!
//! v1 filters by entity id and/or a severity floor only. Rich condition
//! predicates (data-value thresholds, debounce, boolean composition) are
//! deferred to issue #87.

use std::collections::{BTreeMap, BTreeSet};
use std::convert::Infallible;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Json;
use axum::extract::{FromRef, Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::sse::{Event, Sse};
use serde::{Deserialize, Serialize};
use taktora_medkit_gateway::view::collection_segment;
use taktora_medkit_gateway::{FaultStatusFilter, Manifest, MergedView};
use taktora_medkit_model::{
    EntityKind, FaultEvent, FaultEventMeta, FaultSummary, GenericError, Health, Severity,
};
use taktora_medkit_provider::{Provider, severity_to_health};
use tokio::sync::{broadcast, watch};
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;

use crate::error::ApiError;
use crate::locks::LockRegistry;

/// How many change events the broadcast channel buffers for a slow subscriber.
const EVENT_BUFFER: usize = 256;

/// One broadcast item: a sequence id plus the golden-shaped [`FaultEvent`].
#[derive(Clone, Debug)]
pub struct StreamEvent {
    /// Monotonic SSE `id:`.
    pub id: u64,
    /// The golden-shaped event payload.
    pub event: FaultEvent,
}

/// The request body for `POST /api/v1/triggers`: a minimal subscription filter.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct TriggerSpec {
    /// Match only events scoped to this entity id (any entity when absent).
    #[serde(default)]
    pub entity_id: Option<String>,
    /// Match only faults at or above this numeric severity (any when absent).
    #[serde(default)]
    pub severity: Option<u8>,
}

/// A registered trigger: a [`TriggerSpec`] plus its assigned id.
#[derive(Clone, Debug, Serialize)]
pub struct Trigger {
    /// The server-assigned trigger id.
    pub id: String,
    /// Entity-scope filter, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_id: Option<String>,
    /// Severity-floor filter, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<u8>,
}

impl Trigger {
    /// Whether `event` satisfies this trigger's (minimal) filter.
    fn matches(&self, event: &FaultEvent) -> bool {
        self.entity_id
            .as_ref()
            .is_none_or(|e| *e == event.x_medkit.entity_id)
            && self.severity.is_none_or(|s| event.fault.severity >= s)
    }
}

/// The list envelope for `GET /api/v1/triggers`.
#[derive(Serialize)]
pub struct TriggerList {
    items: Vec<Trigger>,
    #[serde(rename = "x-medkit")]
    x_medkit: TriggerListMeta,
}

#[derive(Serialize)]
pub struct TriggerListMeta {
    total_count: usize,
}

/// The in-memory trigger registry.
#[derive(Debug, Default)]
pub struct TriggerStore {
    next: u64,
    triggers: Vec<Trigger>,
}

impl TriggerStore {
    fn create(&mut self, spec: TriggerSpec) -> Trigger {
        self.next += 1;
        let trigger = Trigger {
            id: format!("trg-{}", self.next),
            entity_id: spec.entity_id,
            severity: spec.severity,
        };
        self.triggers.push(trigger.clone());
        trigger
    }

    fn get(&self, id: &str) -> Option<Trigger> {
        self.triggers.iter().find(|t| t.id == id).cloned()
    }

    fn remove(&mut self, id: &str) -> bool {
        let before = self.triggers.len();
        self.triggers.retain(|t| t.id != id);
        self.triggers.len() != before
    }

    /// Whether any registered trigger matches `event`.
    fn any_matches(&self, event: &FaultEvent) -> bool {
        self.triggers.iter().any(|t| t.matches(event))
    }
}

/// The composite handler state: the (hot-swappable) read-model, the trigger
/// registry, and the change-event broadcast.
///
/// The read-core handlers extract the live [`MergedView`] snapshot via the
/// [`FromRef`] impl below, so they are oblivious to the hot-swap.
#[derive(Clone)]
pub struct ServerState {
    view: watch::Receiver<Arc<MergedView>>,
    triggers: Arc<Mutex<TriggerStore>>,
    events: broadcast::Sender<StreamEvent>,
    locks: Arc<LockRegistry>,
}

impl ServerState {
    /// The diagnostic-coordination lock registry behind the `…/{id}/locks`
    /// routes (`BB_0113`, issue #149).
    pub fn locks(&self) -> &LockRegistry {
        &self.locks
    }
}

impl ServerState {
    /// A state with no refresh loop: serves a fixed `view`, supports trigger CRUD,
    /// and exposes an SSE endpoint that simply stays quiet (nothing diffs it).
    pub fn detached(view: Arc<MergedView>) -> Self {
        let (_tx, rx) = watch::channel(view);
        // `_tx` is dropped here; `rx.borrow()` still returns the seeded value.
        let (events, _) = broadcast::channel(EVENT_BUFFER);
        Self::new(rx, events)
    }

    fn new(view: watch::Receiver<Arc<MergedView>>, events: broadcast::Sender<StreamEvent>) -> Self {
        Self {
            view,
            triggers: Arc::new(Mutex::new(TriggerStore::default())),
            events,
            locks: Arc::new(LockRegistry::system()),
        }
    }
}

impl FromRef<ServerState> for Arc<MergedView> {
    fn from_ref(state: &ServerState) -> Self {
        state.view.borrow().clone()
    }
}

fn trigger_not_found(id: &str) -> GenericError {
    GenericError {
        error_code: "trigger-not-found".to_owned(),
        message: "Trigger not found".to_owned(),
        parameters: BTreeMap::from([("trigger_id".to_owned(), id.to_owned())]),
    }
}

// ---- CRUD handlers ---------------------------------------------------------

pub async fn create_trigger(
    State(state): State<ServerState>,
    Json(spec): Json<TriggerSpec>,
) -> impl IntoResponse {
    let trigger = state
        .triggers
        .lock()
        .expect("trigger store poisoned")
        .create(spec);
    (StatusCode::CREATED, Json(trigger))
}

pub async fn list_triggers(State(state): State<ServerState>) -> Json<TriggerList> {
    let items = state
        .triggers
        .lock()
        .expect("trigger store poisoned")
        .triggers
        .clone();
    let total_count = items.len();
    Json(TriggerList {
        items,
        x_medkit: TriggerListMeta { total_count },
    })
}

pub async fn get_trigger(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> Result<Json<Trigger>, ApiError> {
    state
        .triggers
        .lock()
        .expect("trigger store poisoned")
        .get(&id)
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(trigger_not_found(&id)))
}

pub async fn delete_trigger(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    if state
        .triggers
        .lock()
        .expect("trigger store poisoned")
        .remove(&id)
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound(trigger_not_found(&id)))
    }
}

// ---- SSE stream ------------------------------------------------------------

/// Render a [`StreamEvent`] into the golden SSE frame: `id`, then `event`, then a
/// single-line `data` JSON — the exact field order of the captured golden.
fn render(event: &StreamEvent) -> Event {
    Event::default()
        .id(event.id.to_string())
        .event(&event.event.event_type)
        .json_data(&event.event)
        .expect("FaultEvent serializes to JSON")
}

/// `GET /api/v1/triggers/events` — the SSE stream of change events matching any
/// registered trigger (`REQ_0934`).
pub async fn events_stream(State(state): State<ServerState>) -> impl IntoResponse {
    let receiver = state.events.subscribe();
    let triggers = Arc::clone(&state.triggers);
    let stream = BroadcastStream::new(receiver).filter_map(move |result| {
        let event = result.ok()?;
        let pass = triggers
            .lock()
            .expect("trigger store poisoned")
            .any_matches(&event.event);
        pass.then(|| Ok::<_, Infallible>(render(&event)))
    });
    Sse::new(stream)
}

// ---- Refresh-and-diff loop -------------------------------------------------

/// Re-poll `provider` every `cadence`, hot-swap the served view, and broadcast the
/// diff against the previous view as change events (`REQ_0930`, `REQ_0931`).
///
/// Runs off the request path on the tokio side; detached by the caller.
pub async fn refresh_loop<P: Provider>(
    provider: P,
    manifest: Option<Manifest>,
    initial: Arc<MergedView>,
    view_tx: watch::Sender<Arc<MergedView>>,
    events: broadcast::Sender<StreamEvent>,
    cadence: Duration,
) {
    let seq = AtomicU64::new(1);
    let mut prev = initial;
    let mut ticker = tokio::time::interval(cadence);
    loop {
        ticker.tick().await;
        let next = Arc::new(MergedView::from_snapshot_with_manifest(
            provider.snapshot(),
            manifest.clone(),
        ));
        for event in diff_events(&prev, &next, &seq) {
            let _ = events.send(event);
        }
        let _ = view_tx.send(Arc::clone(&next));
        prev = next;
    }
}

/// A per-entity index `entity_id -> (kind, fault_code -> summary)` over the view's
/// public resolvers (entities carrying no fault are omitted).
fn fault_index(
    view: &MergedView,
) -> BTreeMap<String, (EntityKind, BTreeMap<String, FaultSummary>)> {
    let mut index = BTreeMap::new();
    for kind in [
        EntityKind::Area,
        EntityKind::Component,
        EntityKind::Function,
        EntityKind::App,
    ] {
        for entity in view.list(kind).items {
            let faults = view
                .entity_faults(kind, &entity.id, FaultStatusFilter::All)
                .map(|list| list.items)
                .unwrap_or_default();
            if faults.is_empty() {
                continue;
            }
            let by_code = faults
                .into_iter()
                .map(|f| (f.fault_code.clone(), f))
                .collect();
            index.insert(entity.id.clone(), (kind, by_code));
        }
    }
    index
}

type FaultsByCode = BTreeMap<String, FaultSummary>;

fn health_of(faults: Option<&FaultsByCode>) -> Health {
    faults
        .into_iter()
        .flatten()
        .filter_map(|(_, f)| Severity::from_wire_value(f.severity))
        .map(severity_to_health)
        .max()
        .unwrap_or(Health::Ok)
}

fn worst_fault(faults: Option<&FaultsByCode>) -> Option<FaultSummary> {
    faults
        .into_iter()
        .flatten()
        .map(|(_, f)| f)
        .max_by_key(|f| f.severity)
        .cloned()
}

fn make_event(
    event_type: &str,
    kind: EntityKind,
    entity_id: &str,
    fault: &FaultSummary,
    seq: &AtomicU64,
) -> StreamEvent {
    StreamEvent {
        id: seq.fetch_add(1, Ordering::SeqCst),
        event: FaultEvent {
            event_type: event_type.to_owned(),
            fault: fault.clone(),
            timestamp: fault.last_occurred,
            x_medkit: FaultEventMeta {
                entity_id: entity_id.to_owned(),
                entity_type: collection_segment(kind).to_owned(),
            },
        },
    }
}

/// Diff two successive views into the taktora change-event vocabulary:
/// `fault_raised` for a newly-present `(entity, fault_code)`, `fault_cleared` for
/// one that vanished, and `health_changed` when an entity's worst-wins health
/// level moves (`REQ_0931`, `REQ_0932`).
pub fn diff_events(prev: &MergedView, next: &MergedView, seq: &AtomicU64) -> Vec<StreamEvent> {
    let prev_index = fault_index(prev);
    let next_index = fault_index(next);
    let mut events = Vec::new();

    for (entity_id, (kind, next_faults)) in &next_index {
        let prev_faults = prev_index.get(entity_id).map(|(_, f)| f);
        for (code, fault) in next_faults {
            if !prev_faults.is_some_and(|pf| pf.contains_key(code)) {
                events.push(make_event("fault_raised", *kind, entity_id, fault, seq));
            }
        }
    }

    for (entity_id, (kind, prev_faults)) in &prev_index {
        let next_faults = next_index.get(entity_id).map(|(_, f)| f);
        for (code, fault) in prev_faults {
            if !next_faults.is_some_and(|nf| nf.contains_key(code)) {
                events.push(make_event("fault_cleared", *kind, entity_id, fault, seq));
            }
        }
    }

    let ids: BTreeSet<&String> = prev_index.keys().chain(next_index.keys()).collect();
    for entity_id in ids {
        let prev_entry = prev_index.get(entity_id);
        let next_entry = next_index.get(entity_id);
        let prev_faults = prev_entry.map(|(_, f)| f);
        let next_faults = next_entry.map(|(_, f)| f);
        if health_of(prev_faults) == health_of(next_faults) {
            continue;
        }
        let Some((kind, _)) = next_entry.or(prev_entry) else {
            continue;
        };
        if let Some(fault) = worst_fault(next_faults).or_else(|| worst_fault(prev_faults)) {
            events.push(make_event("health_changed", *kind, entity_id, &fault, seq));
        }
    }

    events
}

/// Build the composite state plus the live channel ends for the refresh loop.
pub fn live_state(
    initial: Arc<MergedView>,
) -> (
    ServerState,
    watch::Sender<Arc<MergedView>>,
    broadcast::Sender<StreamEvent>,
) {
    let (view_tx, view_rx) = watch::channel(initial);
    let (events, _) = broadcast::channel(EVENT_BUFFER);
    let state = ServerState::new(view_rx, events.clone());
    (state, view_tx, events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use taktora_medkit_model::{Entity, EntityMeta, Ros2Ref};
    use taktora_medkit_provider::MockProvider;

    fn app(id: &str) -> Entity {
        Entity {
            href: format!("/api/v1/apps/{id}"),
            id: id.to_owned(),
            name: id.to_owned(),
            kind: EntityKind::App,
            parent_id: None,
            description: None,
            x_medkit: Some(EntityMeta {
                ros2: Some(Ros2Ref {
                    node: format!("/{id}"),
                }),
                ..EntityMeta::default()
            }),
        }
    }

    fn fault(code: &str, severity: Severity) -> FaultSummary {
        FaultSummary {
            description: code.to_owned(),
            fault_code: code.to_owned(),
            first_occurred: 1.0,
            last_occurred: 2.0,
            occurrence_count: 1,
            reporting_sources: vec![],
            severity: severity.wire_value(),
            severity_label: format!("{severity:?}").to_uppercase(),
            status: "CONFIRMED".to_owned(),
        }
    }

    fn view(faults: &[(&str, Severity)]) -> MergedView {
        let mut provider = MockProvider::new().with_entity(app("gw"));
        for (code, sev) in faults {
            provider = provider.with_fault("gw", fault(code, *sev));
        }
        MergedView::from_snapshot(provider.snapshot())
    }

    /// `REQ_0931` — a newly-present fault diffs to `fault_raised`, carrying the
    /// golden-shaped entity scoping; clearing it diffs to `fault_cleared`.
    #[test]
    fn diff_raises_and_clears_faults() {
        let seq = AtomicU64::new(1);
        let healthy = view(&[]);
        let faulted = view(&[("BRAKE", Severity::Error)]);

        let raised = diff_events(&healthy, &faulted, &seq);
        assert!(raised.iter().any(|e| e.event.event_type == "fault_raised"
            && e.event.fault.fault_code == "BRAKE"
            && e.event.x_medkit.entity_id == "gw"
            && e.event.x_medkit.entity_type == "apps"));

        let cleared = diff_events(&faulted, &healthy, &seq);
        assert!(
            cleared
                .iter()
                .any(|e| e.event.event_type == "fault_cleared"
                    && e.event.fault.fault_code == "BRAKE")
        );
    }

    /// `REQ_0932` — an entity's health-level transition diffs to `health_changed`,
    /// carrying the fault that drove it.
    #[test]
    fn diff_emits_health_changed_on_transition() {
        let seq = AtomicU64::new(1);
        let ok = view(&[]);
        let err = view(&[("BRAKE", Severity::Error)]);
        let events = diff_events(&ok, &err, &seq);
        let health = events
            .iter()
            .find(|e| e.event.event_type == "health_changed")
            .expect("health_changed emitted");
        assert_eq!(health.event.fault.fault_code, "BRAKE");
        assert_eq!(health.event.x_medkit.entity_id, "gw");
    }

    /// An unchanged view diffs to no events.
    #[test]
    fn diff_of_identical_views_is_empty() {
        let seq = AtomicU64::new(1);
        let v = view(&[("BRAKE", Severity::Error)]);
        assert!(diff_events(&v, &v, &seq).is_empty());
    }

    /// `REQ_0933` — the minimal trigger filter matches by entity and severity floor.
    #[test]
    fn trigger_filters_by_entity_and_severity() {
        let event = make_event(
            "fault_raised",
            EntityKind::App,
            "gw",
            &fault("BRAKE", Severity::Error),
            &AtomicU64::new(1),
        )
        .event;

        let any = Trigger {
            id: "t".to_owned(),
            entity_id: None,
            severity: None,
        };
        assert!(any.matches(&event));

        let entity_match = Trigger {
            id: "t".to_owned(),
            entity_id: Some("gw".to_owned()),
            severity: None,
        };
        assert!(entity_match.matches(&event));

        let entity_miss = Trigger {
            id: "t".to_owned(),
            entity_id: Some("other".to_owned()),
            severity: None,
        };
        assert!(!entity_miss.matches(&event));

        let severity_floor = Trigger {
            id: "t".to_owned(),
            entity_id: None,
            severity: Some(Severity::Critical.wire_value()),
        };
        assert!(!severity_floor.matches(&event));
    }

    /// The store round-trips create / get / remove.
    #[test]
    fn store_round_trips() {
        let mut store = TriggerStore::default();
        let created = store.create(TriggerSpec {
            entity_id: Some("gw".to_owned()),
            severity: Some(2),
        });
        assert!(store.get(&created.id).is_some());
        assert!(store.remove(&created.id));
        assert!(store.get(&created.id).is_none());
        assert!(!store.remove(&created.id));
    }

    /// `FromRef` exposes the live view snapshot to the read-core handlers.
    #[test]
    fn from_ref_yields_live_view() {
        let state = ServerState::detached(Arc::new(view(&[])));
        let extracted = Arc::<MergedView>::from_ref(&state);
        assert!(extracted.entity("gw").is_some());
    }
}
