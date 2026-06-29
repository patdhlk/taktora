//! Cyclic subscriptions: periodic data-sampling subscriptions with an SSE event
//! stream (`REQ_0977`).
//!
//! A cyclic subscription is the data-sampling analogue of a trigger: where a
//! trigger filters the fault **diff** stream, a cyclic subscription periodically
//! **samples** an entity's data tree (through the
//! [`MergedView::data`](taktora_medkit_gateway::MergedView::data) resolver) and
//! pushes each sample over SSE. The CRUD surface mirrors the entity-scoped
//! triggers ([`crate::triggers::trigger_routes`]): a subscription created under
//! `…/{id}/cyclic-subscriptions` is pinned to the path entity, the list is
//! filtered to it, and the per-subscription `…/events` SSE stream emits that
//! entity's data on the subscription's cadence.
//!
//! Unlike the trigger stream — a shared broadcast diffed off the refresh loop —
//! the sampler is a **self-contained periodic stream**: each connection drives
//! its own [`IntervalStream`] and reads the current data each tick. v1 captures
//! the [`MergedView`] snapshot at connect; refreshing against the hot-swapped
//! live view each tick is a future refinement.

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::get;
use serde::{Deserialize, Serialize};
use serde_json::json;
use taktora_medkit_gateway::MergedView;
use taktora_medkit_gateway::view::{API_BASE, collection_segment};
use taktora_medkit_model::{EntityKind, GenericError};
use tokio_stream::wrappers::IntervalStream;
use tokio_stream::{Stream, StreamExt};

use crate::error::ApiError;
use crate::triggers::ServerState;

/// The sample period applied when a subscription declares no `interval_ms`.
const DEFAULT_INTERVAL_MS: u64 = 1000;

/// The floor a sample period is clamped to, so a `0` (or tiny) `interval_ms`
/// cannot busy-loop the sampler.
const MIN_INTERVAL_MS: u64 = 50;

/// The SSE keep-alive cadence: a `:keepalive` comment every 30s holds the
/// connection open through idle periods and proxies.
const KEEPALIVE_SECS: u64 = 30;

/// The request body for `POST …/cyclic-subscriptions`: which data subtree to
/// sample and how often.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct CyclicSpec {
    /// The data subtree to sample (a `/`-navigated topic path); the whole data
    /// tree when absent.
    #[serde(default)]
    pub data_path: Option<String>,
    /// The sample period in milliseconds (defaults to [`DEFAULT_INTERVAL_MS`]).
    #[serde(default)]
    pub interval_ms: Option<u64>,
}

/// A registered cyclic subscription: a [`CyclicSpec`] resolved against the path
/// entity, plus its assigned id.
#[derive(Clone, Debug, Serialize)]
pub struct CyclicSubscription {
    /// The server-assigned subscription id (`cyc-{n}`).
    pub id: String,
    /// The entity this subscription samples (pinned to the path entity).
    pub entity_id: String,
    /// The sampled data subtree, if scoped to one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_path: Option<String>,
    /// The sample period in milliseconds.
    pub interval_ms: u64,
}

/// The list envelope for `GET …/cyclic-subscriptions`.
#[derive(Serialize)]
pub struct CyclicList {
    items: Vec<CyclicSubscription>,
    #[serde(rename = "x-medkit")]
    x_medkit: CyclicListMeta,
}

#[derive(Serialize)]
pub struct CyclicListMeta {
    total_count: usize,
}

/// The in-memory cyclic-subscription registry, mirroring
/// [`TriggerStore`](crate::triggers).
#[derive(Debug, Default)]
pub struct CyclicStore {
    next: u64,
    subscriptions: Vec<CyclicSubscription>,
}

impl CyclicStore {
    /// Register a subscription pinned to `entity_id`, assigning a fresh id.
    fn create(&mut self, entity_id: String, spec: CyclicSpec) -> CyclicSubscription {
        self.next += 1;
        let subscription = CyclicSubscription {
            id: format!("cyc-{}", self.next),
            entity_id,
            data_path: spec.data_path,
            interval_ms: spec.interval_ms.unwrap_or(DEFAULT_INTERVAL_MS),
        };
        self.subscriptions.push(subscription.clone());
        subscription
    }

    fn get(&self, id: &str) -> Option<CyclicSubscription> {
        self.subscriptions.iter().find(|s| s.id == id).cloned()
    }

    /// Replace the spec of an existing subscription (`PUT`); `None` if absent.
    /// The pinned `entity_id` is preserved.
    fn update(&mut self, id: &str, spec: CyclicSpec) -> Option<CyclicSubscription> {
        let subscription = self.subscriptions.iter_mut().find(|s| s.id == id)?;
        subscription.data_path = spec.data_path;
        subscription.interval_ms = spec.interval_ms.unwrap_or(DEFAULT_INTERVAL_MS);
        Some(subscription.clone())
    }

    fn remove(&mut self, id: &str) -> bool {
        let before = self.subscriptions.len();
        self.subscriptions.retain(|s| s.id != id);
        self.subscriptions.len() != before
    }

    /// The subscriptions scoped to `entity_id`, for the entity-scoped list.
    fn list_for_entity(&self, entity_id: &str) -> Vec<CyclicSubscription> {
        self.subscriptions
            .iter()
            .filter(|s| s.entity_id == entity_id)
            .cloned()
            .collect()
    }
}

fn subscription_not_found(id: &str) -> GenericError {
    GenericError {
        error_code: "cyclic-subscription-not-found".to_owned(),
        message: "Cyclic subscription not found".to_owned(),
        parameters: BTreeMap::from([("subscription_id".to_owned(), id.to_owned())]),
    }
}

// ---- Store adapters (keep the route closures one-liners) -------------------

fn list_for_entity(state: &ServerState, entity_id: &str) -> Json<CyclicList> {
    let items = state
        .cyclic()
        .lock()
        .expect("cyclic store poisoned")
        .list_for_entity(entity_id);
    let total_count = items.len();
    Json(CyclicList {
        items,
        x_medkit: CyclicListMeta { total_count },
    })
}

fn create_for_entity(
    state: &ServerState,
    entity_id: String,
    spec: CyclicSpec,
) -> impl IntoResponse + use<> {
    let subscription = state
        .cyclic()
        .lock()
        .expect("cyclic store poisoned")
        .create(entity_id, spec);
    (StatusCode::CREATED, Json(subscription))
}

/// Fetch a subscription and confirm it is scoped to `entity_id`; a missing
/// subscription or one owned by a different entity is a `404` (the entity-scoped
/// view never reveals another entity's subscriptions).
fn scoped_subscription(
    state: &ServerState,
    entity_id: &str,
    subscription_id: &str,
) -> Result<CyclicSubscription, ApiError> {
    state
        .cyclic()
        .lock()
        .expect("cyclic store poisoned")
        .get(subscription_id)
        .filter(|s| s.entity_id == entity_id)
        .ok_or_else(|| ApiError::NotFound(subscription_not_found(subscription_id)))
}

fn update_scoped(
    state: &ServerState,
    entity_id: &str,
    subscription_id: &str,
    spec: CyclicSpec,
) -> Result<Json<CyclicSubscription>, ApiError> {
    scoped_subscription(state, entity_id, subscription_id)?;
    state
        .cyclic()
        .lock()
        .expect("cyclic store poisoned")
        .update(subscription_id, spec)
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(subscription_not_found(subscription_id)))
}

fn delete_scoped(
    state: &ServerState,
    entity_id: &str,
    subscription_id: &str,
) -> Result<StatusCode, ApiError> {
    scoped_subscription(state, entity_id, subscription_id)?;
    state
        .cyclic()
        .lock()
        .expect("cyclic store poisoned")
        .remove(subscription_id);
    Ok(StatusCode::NO_CONTENT)
}

// ---- The periodic data-sampling SSE stream ---------------------------------

/// Build the SSE stream that samples `view.data(kind, id, sub.data_path)` every
/// `sub.interval_ms` (clamped to [`MIN_INTERVAL_MS`]), emitting each sample as an
/// `event: sample` frame with a monotonic `id:`.
///
/// The first interval tick fires immediately, so a sample lands at connect. The
/// captured `view` snapshot is read each tick; live-view refresh is a future
/// refinement.
fn sample_stream(
    view: Arc<MergedView>,
    kind: EntityKind,
    id: String,
    sub: CyclicSubscription,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let period = Duration::from_millis(sub.interval_ms.max(MIN_INTERVAL_MS));
    let data_path = sub.data_path;
    let mut next_id: u64 = 0;
    let stream = IntervalStream::new(tokio::time::interval(period)).map(move |_| {
        next_id += 1;
        let value = view
            .data(kind, &id, data_path.as_deref())
            .unwrap_or_else(|_| json!({}));
        Ok::<_, Infallible>(
            Event::default()
                .id(next_id.to_string())
                .event("sample")
                .json_data(&value)
                .expect("sampled data Value serializes to JSON"),
        )
    });
    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(KEEPALIVE_SECS))
            .text("keepalive"),
    )
}

// ---- Entity-scoped cyclic-subscription routes (`REQ_0977`) -----------------

/// The cyclic-subscription routes mounted per entity
/// (`/{collection}/{id}/cyclic-subscriptions…`). A subscription created here is
/// pinned to the path entity, the list is filtered to it, and the per-resource
/// `…/events` SSE stream samples that entity's data on the subscription's
/// cadence. The named adapters above keep this a flat wiring of routes.
pub fn cyclic_routes(kind: EntityKind) -> Router<ServerState> {
    let base = format!(
        "{API_BASE}/{}/{{id}}/cyclic-subscriptions",
        collection_segment(kind)
    );
    let item = format!("{base}/{{sub_id}}");
    let events = format!("{item}/events");
    Router::new()
        .route(
            &base,
            get(
                |State(state): State<ServerState>, Path(id): Path<String>| async move {
                    list_for_entity(&state, &id)
                },
            )
            .post(
                |State(state): State<ServerState>,
                 Path(id): Path<String>,
                 Json(spec): Json<CyclicSpec>| async move {
                    create_for_entity(&state, id, spec)
                },
            ),
        )
        .route(
            &item,
            get(
                |State(state): State<ServerState>,
                 Path((id, sub_id)): Path<(String, String)>| async move {
                    scoped_subscription(&state, &id, &sub_id).map(Json)
                },
            )
            .put(
                |State(state): State<ServerState>,
                 Path((id, sub_id)): Path<(String, String)>,
                 Json(spec): Json<CyclicSpec>| async move {
                    update_scoped(&state, &id, &sub_id, spec)
                },
            )
            .delete(
                |State(state): State<ServerState>,
                 Path((id, sub_id)): Path<(String, String)>| async move {
                    delete_scoped(&state, &id, &sub_id)
                },
            ),
        )
        .route(
            &events,
            get(
                move |State(state): State<ServerState>,
                      State(view): State<Arc<MergedView>>,
                      Path((id, sub_id)): Path<(String, String)>| async move {
                    let sub = scoped_subscription(&state, &id, &sub_id)?;
                    Ok::<_, ApiError>(sample_stream(view, kind, id, sub))
                },
            ),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The store round-trips create / get / update / remove and pins the entity.
    #[test]
    fn store_round_trips() {
        let mut store = CyclicStore::default();
        let created = store.create(
            "gw".to_owned(),
            CyclicSpec {
                data_path: Some("cpu".to_owned()),
                interval_ms: Some(30),
            },
        );
        assert_eq!(created.entity_id, "gw");
        assert_eq!(created.interval_ms, 30);
        assert!(store.get(&created.id).is_some());

        let updated = store
            .update(
                &created.id,
                CyclicSpec {
                    data_path: None,
                    interval_ms: None,
                },
            )
            .expect("update existing");
        // The pinned entity survives an update; the spec is replaced.
        assert_eq!(updated.entity_id, "gw");
        assert_eq!(updated.interval_ms, DEFAULT_INTERVAL_MS);
        assert!(updated.data_path.is_none());

        assert!(store.remove(&created.id));
        assert!(store.get(&created.id).is_none());
        assert!(!store.remove(&created.id));
    }

    /// The entity-scoped list never leaks another entity's subscriptions.
    #[test]
    fn store_scopes_by_entity() {
        let mut store = CyclicStore::default();
        let a = store.create("app-a".to_owned(), CyclicSpec::default());
        store.create("app-b".to_owned(), CyclicSpec::default());

        let for_a = store.list_for_entity("app-a");
        assert_eq!(for_a.len(), 1);
        assert_eq!(for_a[0].id, a.id);
        assert!(store.list_for_entity("app-c").is_empty());
    }
}
