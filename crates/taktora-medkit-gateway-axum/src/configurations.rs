//! The **configurations** write family: SOVD per-entity configuration storage
//! over HTTP (`REQ_0971`), served through the [`ActionSink`] seam.
//!
//! The contract models a configuration as a keyed JSON document under a target:
//! `GET …/configurations` lists them, `GET …/configurations/{id}` reads one,
//! `PUT …/configurations/{id}` upserts it, and `DELETE` removes one or all.
//! These handlers are thin adapters over [`ActionSink`] — exactly as the
//! operations handlers are — so the in-memory
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
            message: "Configuration not found".to_owned(),
            parameters: params,
        }),
        ActionError::Conflict => ApiError::Conflict(GenericError {
            error_code: "conflict".to_owned(),
            message: "The configuration is not in a writable state".to_owned(),
            parameters: params,
        }),
        ActionError::BadRequest(message) => ApiError::BadRequest(GenericError {
            error_code: "invalid-parameter".to_owned(),
            message,
            parameters: params,
        }),
    }
}

/// The configurations routes for one entity `kind`, mounted under
/// `/{collection}/{id}/configurations` (`REQ_0971`). Mirrors the per-kind
/// operations routes.
pub fn configuration_routes(kind: EntityKind) -> Router<ServerState> {
    let base = format!(
        "{API_BASE}/{}/{{id}}/configurations",
        collection_segment(kind)
    );
    let detail = format!("{base}/{{config_id}}");
    Router::new()
        .route(
            &base,
            get(
                move |State(state): State<ServerState>, Path(id): Path<String>| async move {
                    collection(state.actions().configurations(&ResourceRef::new(kind, id)))
                },
            )
            .delete(
                move |State(state): State<ServerState>, Path(id): Path<String>| async move {
                    let target = ResourceRef::new(kind, id);
                    state
                        .actions()
                        .delete_configurations(&target)
                        .map(|()| StatusCode::NO_CONTENT)
                        .map_err(|e| action_error(e, kind, &target.id))
                },
            ),
        )
        .route(
            &detail,
            get(
                move |State(state): State<ServerState>,
                      Path((id, config_id)): Path<(String, String)>| async move {
                    let target = ResourceRef::new(kind, id);
                    state
                        .actions()
                        .configuration(&target, &config_id)
                        .map(Json)
                        .map_err(|e| action_error(e, kind, &target.id))
                },
            )
            .put(
                move |State(state): State<ServerState>,
                      Path((id, config_id)): Path<(String, String)>,
                      body: Json<Value>| async move {
                    let target = ResourceRef::new(kind, id);
                    let Json(value) = body;
                    state
                        .actions()
                        .set_configuration(&target, &config_id, value)
                        .map(Json)
                        .map_err(|e| action_error(e, kind, &target.id))
                },
            )
            .delete(
                move |State(state): State<ServerState>,
                      Path((id, config_id)): Path<(String, String)>| async move {
                    let target = ResourceRef::new(kind, id);
                    state
                        .actions()
                        .delete_configuration(&target, &config_id)
                        .map(|()| StatusCode::NO_CONTENT)
                        .map_err(|e| action_error(e, kind, &target.id))
                },
            ),
        )
}
