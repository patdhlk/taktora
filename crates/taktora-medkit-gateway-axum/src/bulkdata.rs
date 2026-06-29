//! The **bulk-data** write family: SOVD per-entity opaque file storage over
//! HTTP (`REQ_0972`), served through the [`ActionSink`] seam.
//!
//! The contract models bulk-data as files grouped into categories under a
//! target: `GET …/bulk-data` lists the categories, `GET …/bulk-data/{category}`
//! lists the file descriptors in one, `POST …/bulk-data/{category}` uploads a
//! file (the raw request body), and `GET`/`DELETE
//! …/bulk-data/{category}/{file}` download or remove one. These handlers are
//! thin adapters over [`ActionSink`] — exactly as the configurations handlers
//! are — so the in-memory
//! [`SimActionSink`](taktora_medkit_provider::SimActionSink) backs them in tests
//! and the walking skeleton, and a real binding backs them later without any
//! handler change. v1 is a pure in-memory simulation: the body is stored
//! opaquely as bytes (no multipart parsing) and no real effect is performed.
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
            message: "Bulk-data file not found".to_owned(),
            parameters: params,
        }),
        ActionError::Conflict => ApiError::Conflict(GenericError {
            error_code: "conflict".to_owned(),
            message: "The bulk-data resource is not in a writable state".to_owned(),
            parameters: params,
        }),
        ActionError::BadRequest(message) => ApiError::BadRequest(GenericError {
            error_code: "invalid-parameter".to_owned(),
            message,
            parameters: params,
        }),
    }
}

/// The bulk-data routes for one entity `kind`, mounted under
/// `/{collection}/{id}/bulk-data` (`REQ_0972`). Mirrors the per-kind
/// configurations routes.
pub fn bulk_data_routes(kind: EntityKind) -> Router<ServerState> {
    let base = format!("{API_BASE}/{}/{{id}}/bulk-data", collection_segment(kind));
    let category = format!("{base}/{{category_id}}");
    let file = format!("{category}/{{file_id}}");
    Router::new()
        .route(
            &base,
            get(
                move |State(state): State<ServerState>, Path(id): Path<String>| async move {
                    collection(state.actions().bulk_categories(&ResourceRef::new(kind, id)))
                },
            ),
        )
        .route(
            &category,
            get(
                move |State(state): State<ServerState>,
                      Path((id, category_id)): Path<(String, String)>| async move {
                    collection(
                        state
                            .actions()
                            .bulk_descriptors(&ResourceRef::new(kind, id), &category_id),
                    )
                },
            )
            .post(
                move |State(state): State<ServerState>,
                      Path((id, category_id)): Path<(String, String)>,
                      bytes: Bytes| async move {
                    let target = ResourceRef::new(kind, id);
                    state
                        .actions()
                        .upload_bulk(&target, &category_id, bytes.to_vec())
                        .map(|descriptor| (StatusCode::CREATED, Json(descriptor)))
                        .map_err(|e| action_error(e, kind, &target.id))
                },
            ),
        )
        .route(
            &file,
            get(
                move |State(state): State<ServerState>,
                      Path((id, category_id, file_id)): Path<(String, String, String)>| async move {
                    let target = ResourceRef::new(kind, id);
                    state
                        .actions()
                        .download_bulk(&target, &category_id, &file_id)
                        .map_err(|e| action_error(e, kind, &target.id))
                },
            )
            .delete(
                move |State(state): State<ServerState>,
                      Path((id, category_id, file_id)): Path<(String, String, String)>| async move {
                    let target = ResourceRef::new(kind, id);
                    state
                        .actions()
                        .delete_bulk(&target, &category_id, &file_id)
                        .map(|()| StatusCode::NO_CONTENT)
                        .map_err(|e| action_error(e, kind, &target.id))
                },
            ),
        )
}
