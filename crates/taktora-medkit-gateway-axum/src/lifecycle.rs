//! The **lifecycle-status** write family: SOVD per-entity start/restart/shutdown
//! transitions over HTTP (`REQ_0975`), served through the [`ActionSink`] seam.
//!
//! The contract models an entity's lifecycle as a `{ "status": "<state>" }`
//! document under a target: `GET …/status` reads the current state (defaulting
//! to `"running"`), and `PUT …/status/{transition}` requests a transition
//! (`start`, `restart`, `shutdown`, `force-restart`, `force-shutdown`),
//! answering `202 Accepted` with the new status. These handlers are thin
//! adapters over [`ActionSink`] — like the configurations handlers — so the
//! in-memory [`SimActionSink`](taktora_medkit_provider::SimActionSink) backs
//! them in tests and the walking skeleton, and a real binding backs them later
//! without any handler change.
//!
//! [`ActionSink`]: taktora_medkit_provider::ActionSink
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
use taktora_medkit_gateway::view::{API_BASE, collection_segment};
use taktora_medkit_model::{EntityKind, GenericError};
use taktora_medkit_provider::{ActionError, ResourceRef};

use crate::error::ApiError;
use crate::triggers::ServerState;

/// Map an [`ActionError`] to a contract-shaped [`ApiError`] with target context.
fn action_error(error: ActionError, kind: EntityKind, id: &str) -> ApiError {
    let params = BTreeMap::from([
        ("entity".to_owned(), collection_segment(kind).to_owned()),
        ("entity_id".to_owned(), id.to_owned()),
    ]);
    match error {
        ActionError::NotFound => ApiError::NotFound(GenericError {
            error_code: "not-found".to_owned(),
            message: "Entity not found".to_owned(),
            parameters: params,
        }),
        ActionError::Conflict => ApiError::Conflict(GenericError {
            error_code: "conflict".to_owned(),
            message: "The entity is not in a transitionable state".to_owned(),
            parameters: params,
        }),
        ActionError::BadRequest(message) => ApiError::BadRequest(GenericError {
            error_code: "invalid-parameter".to_owned(),
            message,
            parameters: params,
        }),
    }
}

/// The lifecycle-status routes for one entity `kind`, mounted under
/// `/{collection}/{id}/status` (`REQ_0975`). Mirrors the per-kind configurations
/// routes.
///
/// `GET …/status` returns the current state; `PUT …/status/{transition}`
/// requests a transition and answers `202 Accepted`. The two paths are distinct
/// (`…/status` vs `…/status/{transition}`), so the GET status route never
/// collides with the transition capture.
pub fn lifecycle_routes(kind: EntityKind) -> Router<ServerState> {
    let base = format!("{API_BASE}/{}/{{id}}/status", collection_segment(kind));
    let transition = format!("{base}/{{transition}}");
    Router::new()
        .route(
            &base,
            get(
                move |State(state): State<ServerState>, Path(id): Path<String>| async move {
                    Json(
                        state
                            .actions()
                            .lifecycle_status(&ResourceRef::new(kind, id)),
                    )
                },
            ),
        )
        .route(
            &transition,
            put(
                move |State(state): State<ServerState>,
                      Path((id, transition)): Path<(String, String)>| async move {
                    let target = ResourceRef::new(kind, id);
                    state
                        .actions()
                        .request_transition(&target, &transition)
                        .map(|status| (StatusCode::ACCEPTED, Json(status)))
                        .map_err(|e| action_error(e, kind, &target.id))
                },
            ),
        )
}
