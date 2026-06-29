//! The **operations** write family: SOVD async action executions over HTTP
//! (`REQ_0969`, `REQ_0970`), served through the [`ActionSink`] seam.
//!
//! The contract models an operation as an async command with an executions
//! sub-resource: `POST …/operations/{op}/executions` starts one (`202`), and the
//! client polls `GET …/executions/{id}` for status and `DELETE`s to cancel. These
//! handlers are thin adapters over [`ActionSink`] — exactly as the read-core
//! handlers are thin adapters over `MergedView` — so the in-memory
//! [`SimActionSink`](taktora_medkit_provider::SimActionSink) backs them in tests
//! and the walking skeleton, and a real binding backs them later without any
//! handler change.
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
use axum::routing::get;
use serde::Serialize;
use serde_json::Value;
use taktora_medkit_gateway::view::{API_BASE, collection_segment};
use taktora_medkit_model::{EntityKind, GenericError};
use taktora_medkit_provider::{ActionError, OperationDef, ResourceRef};

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
            message: "Operation or execution not found".to_owned(),
            parameters: params,
        }),
        ActionError::Conflict => ApiError::Conflict(GenericError {
            error_code: "conflict".to_owned(),
            message: "The execution is not in a cancellable state".to_owned(),
            parameters: params,
        }),
        ActionError::BadRequest(message) => ApiError::BadRequest(GenericError {
            error_code: "invalid-parameter".to_owned(),
            message,
            parameters: params,
        }),
    }
}

/// Find one operation definition in a target's catalogue, or `404`.
fn operation_def(
    state: &ServerState,
    target: &ResourceRef,
    op: &str,
) -> Result<OperationDef, ApiError> {
    state
        .actions()
        .operations(target)
        .into_iter()
        .find(|d| d.id == op)
        .ok_or_else(|| action_error(ActionError::NotFound, target.kind, &target.id))
}

/// The operations routes for one entity `kind`, mounted under
/// `/{collection}/{id}/operations`. Mirrors the per-kind `kind_routes` pattern.
pub fn operation_routes(kind: EntityKind) -> Router<ServerState> {
    let base = format!("{API_BASE}/{}/{{id}}/operations", collection_segment(kind));
    let detail = format!("{base}/{{op}}");
    let execs = format!("{detail}/executions");
    let exec = format!("{execs}/{{exec_id}}");
    Router::new()
        .route(
            &base,
            get(move |State(state): State<ServerState>, Path(id): Path<String>| async move {
                collection(state.actions().operations(&ResourceRef::new(kind, id)))
            }),
        )
        .route(
            &detail,
            get(
                move |State(state): State<ServerState>, Path((id, op)): Path<(String, String)>| async move {
                    let target = ResourceRef::new(kind, id);
                    operation_def(&state, &target, &op).map(Json)
                },
            ),
        )
        .route(
            &execs,
            get(
                move |State(state): State<ServerState>, Path((id, op)): Path<(String, String)>| async move {
                    let target = ResourceRef::new(kind, id);
                    state
                        .actions()
                        .executions(&target, &op)
                        .map(collection)
                        .map_err(|e| action_error(e, kind, &target.id))
                },
            )
            .post(
                move |State(state): State<ServerState>,
                      Path((id, op)): Path<(String, String)>,
                      body: Option<Json<Value>>| async move {
                    let target = ResourceRef::new(kind, id);
                    let args = body.map_or(Value::Null, |Json(v)| v);
                    state
                        .actions()
                        .start_operation(&target, &op, args)
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
                      Path((id, op, exec_id)): Path<(String, String, String)>| async move {
                    let target = ResourceRef::new(kind, id);
                    state
                        .actions()
                        .execution(&target, &op, &exec_id)
                        .map(Json)
                        .map_err(|e| action_error(e, kind, &target.id))
                },
            )
            .put(
                // Update is a benign ack in v1: return the current execution.
                move |State(state): State<ServerState>,
                      Path((id, op, exec_id)): Path<(String, String, String)>| async move {
                    let target = ResourceRef::new(kind, id);
                    state
                        .actions()
                        .execution(&target, &op, &exec_id)
                        .map(Json)
                        .map_err(|e| action_error(e, kind, &target.id))
                },
            )
            .delete(
                move |State(state): State<ServerState>,
                      Path((id, op, exec_id)): Path<(String, String, String)>| async move {
                    let target = ResourceRef::new(kind, id);
                    state
                        .actions()
                        .cancel_execution(&target, &op, &exec_id)
                        .map(|()| StatusCode::NO_CONTENT)
                        .map_err(|e| action_error(e, kind, &target.id))
                },
            ),
        )
}
