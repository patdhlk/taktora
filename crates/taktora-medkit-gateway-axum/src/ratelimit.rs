//! A small in-crate token-bucket rate limiter, applied as an axum middleware.
//!
//! Self-contained so the skeleton carries no governor dependency. The bucket
//! lives off the control path (this is the diagnostics server, not the executor
//! `WaitSet`), so a `std` mutex is fine; over-limit requests answer a
//! contract-shaped `429` (`REQ_0919`).

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Instant;

use axum::Json;
use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use taktora_medkit_model::GenericError;

use crate::config::RateLimit;

/// A shared token bucket: `tokens` refilled at `refill_per_second` up to
/// `capacity`.
#[derive(Debug)]
pub struct TokenBucket {
    capacity: f64,
    refill_per_second: f64,
    state: Mutex<BucketState>,
}

#[derive(Debug)]
struct BucketState {
    tokens: f64,
    last: Instant,
}

impl TokenBucket {
    /// Build a full bucket from a [`RateLimit`].
    #[must_use]
    pub fn new(limit: RateLimit) -> Self {
        let capacity = f64::from(limit.capacity);
        Self {
            capacity,
            refill_per_second: f64::from(limit.refill_per_second),
            state: Mutex::new(BucketState {
                tokens: capacity,
                last: Instant::now(),
            }),
        }
    }

    /// Try to consume one token, refilling for elapsed time first.
    fn try_acquire(&self) -> bool {
        let now = Instant::now();
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let elapsed = now.duration_since(state.last).as_secs_f64();
        state.tokens = elapsed
            .mul_add(self.refill_per_second, state.tokens)
            .min(self.capacity);
        state.last = now;
        if state.tokens >= 1.0 {
            state.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// Middleware: pass the request through, or answer `429` if the bucket is empty.
pub async fn enforce(bucket: &TokenBucket, request: Request, next: Next) -> Response {
    if bucket.try_acquire() {
        next.run(request).await
    } else {
        let body = GenericError {
            error_code: "rate-limited".to_owned(),
            message: "Too many requests".to_owned(),
            parameters: BTreeMap::new(),
        };
        (StatusCode::TOO_MANY_REQUESTS, Json(body)).into_response()
    }
}
