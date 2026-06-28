//! The HTTP error surface: every failure leaves as a contract-shaped
//! [`GenericError`] body so a path-hardcoding client never meets a bare status
//! or a parse error (`REQ_0918`).

use std::collections::BTreeMap;

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use taktora_medkit_gateway::ResolveError;
use taktora_medkit_model::GenericError;

/// A handler failure that renders as a status code plus a [`GenericError`] body.
#[derive(Clone, Debug)]
pub enum ApiError {
    /// The addressed resource does not exist (`404`).
    NotFound(GenericError),
    /// The request was malformed, e.g. an unknown `?status=` value (`400`).
    BadRequest(GenericError),
    /// A deferred family: a recognised SOVD endpoint this skeleton does not yet
    /// implement (`501`). Carries the family and the path for the body.
    NotImplemented {
        /// The deferred SOVD family (e.g. `operations`, `bulk-data`).
        family: String,
        /// The request path that was declined.
        path: String,
    },
}

impl ApiError {
    /// A `501 Not Implemented` for a deferred family at `path`.
    #[must_use]
    pub fn not_implemented(family: impl Into<String>, path: impl Into<String>) -> Self {
        Self::NotImplemented {
            family: family.into(),
            path: path.into(),
        }
    }

    /// A `400 Bad Request` for an unrecognised `?status=` filter value.
    #[must_use]
    pub fn bad_status(value: &str) -> Self {
        Self::BadRequest(GenericError {
            error_code: "invalid-parameter".to_owned(),
            message: "Unknown fault status filter".to_owned(),
            parameters: BTreeMap::from([("status".to_owned(), value.to_owned())]),
        })
    }
}

impl From<ResolveError> for ApiError {
    fn from(error: ResolveError) -> Self {
        match error {
            ResolveError::NotFound(generic) => Self::NotFound(generic),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            Self::NotFound(generic) => (StatusCode::NOT_FOUND, generic),
            Self::BadRequest(generic) => (StatusCode::BAD_REQUEST, generic),
            Self::NotImplemented { family, path } => (
                StatusCode::NOT_IMPLEMENTED,
                GenericError {
                    error_code: "not-implemented".to_owned(),
                    message: format!("The '{family}' family is not implemented"),
                    parameters: BTreeMap::from([
                        ("family".to_owned(), family),
                        ("path".to_owned(), path),
                    ]),
                },
            ),
        };
        (status, Json(body)).into_response()
    }
}
