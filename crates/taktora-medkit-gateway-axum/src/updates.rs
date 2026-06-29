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
use axum::routing::{get, put};
use serde::Serialize;
use serde_json::{Value, json};
use taktora_medkit_gateway::view::API_BASE;
use taktora_medkit_model::GenericError;
use taktora_medkit_provider::ActionError;

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
            message: "The update is not in a transitionable state".to_owned(),
            parameters: params,
        }),
        ActionError::BadRequest(message) => ApiError::BadRequest(GenericError {
            error_code: "invalid-parameter".to_owned(),
            message,
            parameters: params,
        }),
    }
}

/// The global updates routes, mounted under `/updates` (`REQ_0974`). Unlike the
/// per-entity write families, this factory takes no kind and is mounted once.
pub fn update_routes() -> Router<ServerState> {
    let base = format!("{API_BASE}/updates");
    let detail = format!("{base}/{{update_id}}");
    let status = format!("{detail}/status");
    let prepare = format!("{detail}/prepare");
    let execute = format!("{detail}/execute");
    let automated = format!("{detail}/automated");
    Router::new()
        .route(
            &base,
            get(|State(state): State<ServerState>| async move {
                collection(state.actions().updates())
            })
            .post(
                |State(state): State<ServerState>, body: Option<Json<Value>>| async move {
                    let spec = body.map_or(Value::Null, |Json(value)| value);
                    let record = state.actions().register_update(spec);
                    (StatusCode::CREATED, Json(record))
                },
            ),
        )
        .route(
            &detail,
            get(
                |State(state): State<ServerState>, Path(update_id): Path<String>| async move {
                    state
                        .actions()
                        .update(&update_id)
                        .map(Json)
                        .map_err(|e| action_error(e, &update_id))
                },
            )
            .delete(
                |State(state): State<ServerState>, Path(update_id): Path<String>| async move {
                    state
                        .actions()
                        .delete_update(&update_id)
                        .map(|()| StatusCode::NO_CONTENT)
                        .map_err(|e| action_error(e, &update_id))
                },
            ),
        )
        .route(
            &status,
            get(
                |State(state): State<ServerState>, Path(update_id): Path<String>| async move {
                    state
                        .actions()
                        .update(&update_id)
                        .map(|record| Json(json!({ "status": record.status })))
                        .map_err(|e| action_error(e, &update_id))
                },
            ),
        )
        .route(
            &prepare,
            put(
                |State(state): State<ServerState>, Path(update_id): Path<String>| async move {
                    state
                        .actions()
                        .prepare_update(&update_id)
                        .map(|record| (StatusCode::ACCEPTED, Json(record)))
                        .map_err(|e| action_error(e, &update_id))
                },
            ),
        )
        .route(
            &execute,
            put(
                |State(state): State<ServerState>, Path(update_id): Path<String>| async move {
                    state
                        .actions()
                        .execute_update(&update_id)
                        .map(|record| (StatusCode::ACCEPTED, Json(record)))
                        .map_err(|e| action_error(e, &update_id))
                },
            ),
        )
        .route(
            &automated,
            put(
                |State(state): State<ServerState>, Path(update_id): Path<String>| async move {
                    state
                        .actions()
                        .automated_update(&update_id)
                        .map(|record| (StatusCode::ACCEPTED, Json(record)))
                        .map_err(|e| action_error(e, &update_id))
                },
            ),
        )
}
