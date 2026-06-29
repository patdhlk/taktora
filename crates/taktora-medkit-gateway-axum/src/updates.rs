//! The **updates** (software-update) write family: the SOVD update surface over
//! HTTP (`REQ_0974`), served through the [`ActionSink`] seam.
//!
//! Unlike the other write families, updates are **global**: the contract mounts
//! them at the top level (`/api/v1/updates…`), not under an entity, so the route
//! factory takes no entity kind and the [`ActionSink`] methods carry no
//! [`ResourceRef`](taktora_medkit_provider::ResourceRef). `GET …/updates` lists
//! registered updates, `POST …/updates` registers one, and the per-id endpoints
//! read it (`…/{update_id}`, `…/{update_id}/status`) and drive its lifecycle
//! (`…/prepare`, `…/execute`, `…/automated`, `DELETE`). These handlers are thin
//! adapters over [`ActionSink`] — exactly as the other write families are — so
//! the in-memory [`SimActionSink`](taktora_medkit_provider::SimActionSink) backs
//! them in tests and the walking skeleton, and a real binding backs them later
//! without any handler change.
//!
//! # Safety boundary (deferred)
//!
//! The simulation performs no real effect (`ADR_0126`); the write-surface safety
//! gate (`ADR_0119`) re-enters at the [`ActionSink`] seam when a real-effect
//! binding lands, not here.

use std::collections::BTreeMap;

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, put};
use serde::Serialize;
use serde_json::{Value, json};
use taktora_medkit_gateway::view::API_BASE;
use taktora_medkit_model::GenericError;
use taktora_medkit_provider::{ActionError, UpdateRecord};

use crate::error::ApiError;
use crate::triggers::ServerState;

/// A collection envelope (`items` + `x-medkit.total_count`) over any wire item.
#[derive(Serialize)]
struct Collection<T> {
    items: Vec<T>,
    #[serde(rename = "x-medkit")]
    x_medkit: CollectionMeta,
}

#[derive(Serialize)]
struct CollectionMeta {
    total_count: usize,
}

fn collection<T: Serialize>(items: Vec<T>) -> Json<Collection<T>> {
    let total_count = items.len();
    Json(Collection {
        items,
        x_medkit: CollectionMeta { total_count },
    })
}

/// Map an [`ActionError`] to a contract-shaped [`ApiError`] carrying the addressed
/// `update_id` (the updates family has no entity context) — `REQ_0974`.
fn action_error(error: ActionError, update_id: &str) -> ApiError {
    let params = BTreeMap::from([("update_id".to_owned(), update_id.to_owned())]);
    match error {
        ActionError::NotFound => ApiError::NotFound(GenericError {
            error_code: "not-found".to_owned(),
            message: "Update not found".to_owned(),
            parameters: params,
        }),
        ActionError::Conflict => ApiError::Conflict(GenericError {
            error_code: "conflict".to_owned(),
            message: "The update cannot be transitioned from its current state".to_owned(),
            parameters: params,
        }),
        ActionError::BadRequest(message) => ApiError::BadRequest(GenericError {
            error_code: "invalid-parameter".to_owned(),
            message,
            parameters: params,
        }),
    }
}

// ---- Handlers (named, so the route factory stays flat) ---------------------

async fn list_updates(State(state): State<ServerState>) -> Json<Collection<UpdateRecord>> {
    collection(state.actions().updates())
}

async fn register_update(
    State(state): State<ServerState>,
    body: Option<Json<Value>>,
) -> impl IntoResponse {
    let spec = body.map_or(Value::Null, |Json(value)| value);
    (
        StatusCode::CREATED,
        Json(state.actions().register_update(spec)),
    )
}

async fn get_update(
    State(state): State<ServerState>,
    Path(update_id): Path<String>,
) -> Result<Json<UpdateRecord>, ApiError> {
    state
        .actions()
        .update(&update_id)
        .map(Json)
        .map_err(|e| action_error(e, &update_id))
}

async fn delete_update(
    State(state): State<ServerState>,
    Path(update_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state
        .actions()
        .delete_update(&update_id)
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|e| action_error(e, &update_id))
}

async fn update_status(
    State(state): State<ServerState>,
    Path(update_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    state
        .actions()
        .update(&update_id)
        .map(|record| Json(json!({ "status": record.status })))
        .map_err(|e| action_error(e, &update_id))
}

/// Apply a `state → record` transition (prepare / execute / automated), shaping
/// the `202 Accepted` response and mapping errors. Keeps the three transition
/// handlers a single line each.
fn transitioned(
    result: Result<UpdateRecord, ActionError>,
    update_id: &str,
) -> Result<(StatusCode, Json<UpdateRecord>), ApiError> {
    result
        .map(|record| (StatusCode::ACCEPTED, Json(record)))
        .map_err(|e| action_error(e, update_id))
}

async fn prepare_update(
    State(state): State<ServerState>,
    Path(update_id): Path<String>,
) -> Result<(StatusCode, Json<UpdateRecord>), ApiError> {
    transitioned(state.actions().prepare_update(&update_id), &update_id)
}

async fn execute_update(
    State(state): State<ServerState>,
    Path(update_id): Path<String>,
) -> Result<(StatusCode, Json<UpdateRecord>), ApiError> {
    transitioned(state.actions().execute_update(&update_id), &update_id)
}

async fn automated_update(
    State(state): State<ServerState>,
    Path(update_id): Path<String>,
) -> Result<(StatusCode, Json<UpdateRecord>), ApiError> {
    transitioned(state.actions().automated_update(&update_id), &update_id)
}

/// The global updates routes, mounted under `/updates` (`REQ_0974`). Unlike the
/// per-entity write families, this factory takes no kind and is mounted once. The
/// handlers are named (above) so this stays a flat wiring of routes.
pub fn update_routes() -> Router<ServerState> {
    let base = format!("{API_BASE}/updates");
    let detail = format!("{base}/{{update_id}}");
    Router::new()
        .route(&base, get(list_updates).post(register_update))
        .route(&detail, get(get_update).delete(delete_update))
        .route(&format!("{detail}/status"), get(update_status))
        .route(&format!("{detail}/prepare"), put(prepare_update))
        .route(&format!("{detail}/execute"), put(execute_update))
        .route(&format!("{detail}/automated"), put(automated_update))
}
