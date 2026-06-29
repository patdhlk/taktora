//! The **scripts** write family: SOVD per-entity script storage plus async
//! executions over HTTP (`REQ_0973`), served through the [`ActionSink`] seam.
//!
//! Scripts are a hybrid surface: an **upload** side like bulk-data (`POST
//! …/scripts` stores the raw request body and returns its metadata, `GET`/`DELETE
//! …/scripts/{script_id}` read or remove one) and an **executions** side like
//! operations (`POST …/scripts/{script_id}/executions` starts one (`202`), and the
//! client polls `GET …/executions/{id}` for status and `DELETE`s to cancel). The
//! execution sub-resource reuses the operations [`Execution`] wire type. These
//! handlers are thin adapters over [`ActionSink`] — exactly as the operations and
//! bulk-data handlers are — so the in-memory
//! [`SimActionSink`](taktora_medkit_provider::SimActionSink) backs them in tests
//! and the walking skeleton, and a real binding backs them later without any
//! handler change. v1 is a pure in-memory simulation: the body is stored opaquely
//! as bytes (no parsing) and a started execution completes synchronously.
//!
//! # Safety boundary (deferred)
//!
//! The simulation performs no real effect (`ADR_0126`); the write-surface safety
//! gate (`ADR_0119`) re-enters at the [`ActionSink`] seam when a real-effect
//! binding lands, not here.

use std::collections::BTreeMap;

use axum::Json;
use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use serde::Serialize;
use taktora_medkit_gateway::view::{API_BASE, collection_segment};
use taktora_medkit_model::{EntityKind, GenericError};
use taktora_medkit_provider::{ActionError, ResourceRef};

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

/// Map an [`ActionError`] to a contract-shaped [`ApiError`] with target context.
fn action_error(error: ActionError, kind: EntityKind, id: &str) -> ApiError {
    let params = BTreeMap::from([
        ("entity".to_owned(), collection_segment(kind).to_owned()),
        ("entity_id".to_owned(), id.to_owned()),
    ]);
    match error {
        ActionError::NotFound => ApiError::NotFound(GenericError {
            error_code: "not-found".to_owned(),
            message: "Script or execution not found".to_owned(),
            parameters: params,
        }),
        ActionError::Conflict => ApiError::Conflict(GenericError {
            error_code: "conflict".to_owned(),
            message: "The script resource is not in a writable state".to_owned(),
            parameters: params,
        }),
        ActionError::BadRequest(message) => ApiError::BadRequest(GenericError {
            error_code: "invalid-parameter".to_owned(),
            message,
            parameters: params,
        }),
    }
}

/// The scripts routes for one entity `kind`, mounted under
/// `/{collection}/{id}/scripts` (`REQ_0973`). Mirrors the per-kind bulk-data
/// (upload) and operations (executions) routes.
pub fn script_routes(kind: EntityKind) -> Router<ServerState> {
    let base = format!("{API_BASE}/{}/{{id}}/scripts", collection_segment(kind));
    let detail = format!("{base}/{{script_id}}");
    let execs = format!("{detail}/executions");
    let exec = format!("{execs}/{{exec_id}}");
    Router::new()
        .route(
            &base,
            get(
                move |State(state): State<ServerState>, Path(id): Path<String>| async move {
                    collection(state.actions().scripts(&ResourceRef::new(kind, id)))
                },
            )
            .post(
                move |State(state): State<ServerState>,
                      Path(id): Path<String>,
                      bytes: Bytes| async move {
                    let target = ResourceRef::new(kind, id);
                    state
                        .actions()
                        .upload_script(&target, bytes.to_vec())
                        .map(|script| (StatusCode::CREATED, Json(script)))
                        .map_err(|e| action_error(e, kind, &target.id))
                },
            ),
        )
        .route(
            &detail,
            get(
                move |State(state): State<ServerState>,
                      Path((id, script_id)): Path<(String, String)>| async move {
                    let target = ResourceRef::new(kind, id);
                    state
                        .actions()
                        .script(&target, &script_id)
                        .map(Json)
                        .map_err(|e| action_error(e, kind, &target.id))
                },
            )
            .delete(
                move |State(state): State<ServerState>,
                      Path((id, script_id)): Path<(String, String)>| async move {
                    let target = ResourceRef::new(kind, id);
                    state
                        .actions()
                        .delete_script(&target, &script_id)
                        .map(|()| StatusCode::NO_CONTENT)
                        .map_err(|e| action_error(e, kind, &target.id))
                },
            ),
        )
        .route(
            &execs,
            axum::routing::post(
                move |State(state): State<ServerState>,
                      Path((id, script_id)): Path<(String, String)>| async move {
                    let target = ResourceRef::new(kind, id);
                    state
                        .actions()
                        .start_script(&target, &script_id)
                        // Async-accepted: `202` with the (sim-completed) execution.
                        .map(|exec| (StatusCode::ACCEPTED, Json(exec)))
                        .map_err(|e| action_error(e, kind, &target.id))
                },
            ),
        )
        .route(
            &exec,
            get(
                move |State(state): State<ServerState>,
                      Path((id, script_id, exec_id)): Path<(String, String, String)>| async move {
                    let target = ResourceRef::new(kind, id);
                    state
                        .actions()
                        .script_execution(&target, &script_id, &exec_id)
                        .map(Json)
                        .map_err(|e| action_error(e, kind, &target.id))
                },
            )
            .put(
                // Terminate is a benign ack in v1: return the current execution.
                move |State(state): State<ServerState>,
                      Path((id, script_id, exec_id)): Path<(String, String, String)>| async move {
                    let target = ResourceRef::new(kind, id);
                    state
                        .actions()
                        .script_execution(&target, &script_id, &exec_id)
                        .map(Json)
                        .map_err(|e| action_error(e, kind, &target.id))
                },
            )
            .delete(
                move |State(state): State<ServerState>,
                      Path((id, script_id, exec_id)): Path<(String, String, String)>| async move {
                    let target = ResourceRef::new(kind, id);
                    state
                        .actions()
                        .cancel_script_execution(&target, &script_id, &exec_id)
                        .map(|()| StatusCode::NO_CONTENT)
                        .map_err(|e| action_error(e, kind, &target.id))
                },
            ),
        )
}
