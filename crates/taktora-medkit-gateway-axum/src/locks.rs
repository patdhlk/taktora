//! In-memory, off-control-path SOVD lock registry (issue #149, refs #87).
//!
//! Diagnostic-scoped **exclusive access**: a lock coordinates *diagnostic
//! clients against each other* over a single entity, carved out from under the
//! deferred-family `501` fallback. It is the one SOVD write family with **zero
//! control-path coupling** — it guards no safety-critical (SC) resource and adds
//! no edge to the executor/connector binding crates, so the surface stays
//! strictly QM and out of any HARA update (`ADR_0120`). The moment a lock guards
//! an SC resource, the full write-surface safety gate (`ADR_0119`) applies.
//!
//! The registry is a plain `HashMap` behind a `Mutex`, off the request path's
//! control-relevant state. TTL is enforced against an **injectable** [`Clock`]
//! so expiry is deterministic and testable — no test sleeps for real time
//! (`REQ_0941`).
//!
//! Contract shape (`contract/openapi.json`, `/{entity}/{id}/locks`):
//! `lock_expiration` is a **millisecond TTL** in the request but an **RFC3339
//! absolute** instant in the [`Lock`] response, so the registry needs a
//! wall-clock source to format it.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use taktora_medkit_gateway::view::{API_BASE, collection_segment};
use taktora_medkit_model::{EntityKind, GenericError};

use crate::error::ApiError;
use crate::triggers::ServerState;

/// Wall-clock source, injectable so TTL behaviour is deterministic in tests.
///
/// The production [`SystemClock`] reads `SystemTime::now`; unit tests inject a
/// fake clock they advance by hand, so TTL expiry is exercised without a real
/// sleep (`REQ_0941`).
pub trait Clock: Send + Sync {
    /// The current wall-clock instant.
    fn now(&self) -> SystemTime;
}

/// The production [`Clock`]: `SystemTime::now`.
#[derive(Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

/// The `AcquireLockRequest` body for `POST /{entity}/{id}/locks`.
#[derive(Debug, Deserialize)]
pub struct AcquireLockRequest {
    /// Lock lifetime in **milliseconds** from now (required).
    pub lock_expiration: u64,
    /// Supervisor override: evict a lock held by another client.
    #[serde(default)]
    pub break_lock: Option<bool>,
    /// Optional opaque coordination scopes, echoed back on the [`Lock`].
    #[serde(default)]
    pub scopes: Option<Vec<String>>,
}

/// The `ExtendLockRequest` body for `PUT /{entity}/{id}/locks/{lock_id}`.
#[derive(Debug, Deserialize)]
pub struct ExtendLockRequest {
    /// New lock lifetime in **milliseconds** from now (required).
    pub lock_expiration: u64,
}

/// The `Lock` response: `lock_expiration` is the **RFC3339 absolute** instant
/// the lock expires (not the request's millisecond TTL).
#[derive(Debug, Serialize)]
// `lock_expiration` is the contract field name; not a redundant struct prefix.
#[allow(clippy::struct_field_names)]
pub struct Lock {
    /// The server-assigned lock id.
    pub id: String,
    /// Whether the requesting client owns this lock.
    pub owned: bool,
    /// RFC3339 absolute expiry instant.
    pub lock_expiration: String,
    /// Echoed coordination scopes, if any were supplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scopes: Option<Vec<String>>,
}

/// The list envelope for `GET /{entity}/{id}/locks`: `items` plus the
/// `x-medkit.total_count` the collection shape carries.
#[derive(Debug, Serialize)]
pub struct LockList {
    items: Vec<Lock>,
    #[serde(rename = "x-medkit")]
    x_medkit: LockListMeta,
}

#[derive(Debug, Serialize)]
pub struct LockListMeta {
    total_count: usize,
}

/// A lock-lifecycle failure, mapped to a contract-shaped [`ApiError`] by the
/// handlers with the offending resource/lock context attached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockError {
    /// The entity is locked by another client and `break_lock` was not set, or a
    /// non-owner tried to extend/release a held lock (`409`).
    Conflict,
    /// No live lock with that id exists on the entity (missing, wrong id, or
    /// already expired) (`404`).
    NotFound,
}

/// One held lock: its id, the holder (`X-Client-Id`), the absolute expiry, and
/// any coordination scopes (echoed back on reads as well as on acquire).
#[derive(Debug, Clone)]
struct LockEntry {
    lock_id: String,
    holder: String,
    expires_at: SystemTime,
    scopes: Option<Vec<String>>,
}

impl LockEntry {
    /// Render this entry as the contract [`Lock`] from `requester`'s viewpoint:
    /// `owned` is `true` only when `requester` is the holder (a read with no
    /// `X-Client-Id` sees `owned: false`).
    fn to_lock(&self, requester: Option<&str>) -> Lock {
        Lock {
            id: self.lock_id.clone(),
            owned: requester == Some(self.holder.as_str()),
            lock_expiration: format_rfc3339(self.expires_at),
            scopes: self.scopes.clone(),
        }
    }
}

/// The resource a lock is scoped to: an entity kind plus its id.
type ResourceKey = (EntityKind, String);

/// The in-memory lock registry: at most one live lock per `(kind, id)` resource.
pub struct LockRegistry {
    locks: Mutex<HashMap<ResourceKey, LockEntry>>,
    next: AtomicU64,
    clock: Box<dyn Clock>,
}

impl LockRegistry {
    /// A registry driven by the given [`Clock`].
    fn with_clock(clock: Box<dyn Clock>) -> Self {
        Self {
            locks: Mutex::new(HashMap::new()),
            next: AtomicU64::new(1),
            clock,
        }
    }

    /// The production registry, driven by the system wall clock.
    pub fn system() -> Self {
        Self::with_clock(Box::new(SystemClock))
    }

    /// Acquire an exclusive lock on `key` for `holder`.
    ///
    /// A free, expired, or self-held resource is (re)acquired. A resource held
    /// *live* by another client yields [`LockError::Conflict`] unless
    /// `break_lock` is set, which evicts the incumbent (supervisor override).
    fn acquire(
        &self,
        key: ResourceKey,
        holder: String,
        ttl_ms: u64,
        break_lock: bool,
        scopes: Option<Vec<String>>,
    ) -> Result<Lock, LockError> {
        let now = self.clock.now();
        let lock_id = format!("lck-{}", self.next.fetch_add(1, Ordering::SeqCst));
        let expires_at = now + Duration::from_millis(ttl_ms);
        {
            let mut map = self.locks.lock().expect("lock registry poisoned");
            if let Some(existing) = map.get(&key) {
                let live = existing.expires_at > now;
                if live && existing.holder != holder && !break_lock {
                    return Err(LockError::Conflict);
                }
            }
            map.insert(
                key,
                LockEntry {
                    lock_id: lock_id.clone(),
                    holder,
                    expires_at,
                    scopes: scopes.clone(),
                },
            );
        }
        Ok(Lock {
            id: lock_id,
            owned: true,
            lock_expiration: format_rfc3339(expires_at),
            scopes,
        })
    }

    /// List the live locks on `key` from `requester`'s viewpoint (`REQ_0963`).
    ///
    /// At most one lock is live per resource, so this returns a 0- or 1-element
    /// vector; an expired lock is omitted (treated as released).
    fn list_for(&self, key: &ResourceKey, requester: Option<&str>) -> Vec<Lock> {
        let now = self.clock.now();
        let map = self.locks.lock().expect("lock registry poisoned");
        map.get(key)
            .filter(|entry| entry.expires_at > now)
            .map(|entry| entry.to_lock(requester))
            .into_iter()
            .collect()
    }

    /// Fetch a single live lock by id from `requester`'s viewpoint, or `None` if
    /// no live lock with that id is held on `key` (`REQ_0963`).
    fn get_one(&self, key: &ResourceKey, lock_id: &str, requester: Option<&str>) -> Option<Lock> {
        let now = self.clock.now();
        let map = self.locks.lock().expect("lock registry poisoned");
        map.get(key)
            .filter(|entry| entry.lock_id == lock_id && entry.expires_at > now)
            .map(|entry| entry.to_lock(requester))
    }

    /// Extend a live lock owned by `holder` to a new TTL.
    ///
    /// A missing, wrongly-identified, or already-expired lock yields
    /// [`LockError::NotFound`]; a live lock held by another client yields
    /// [`LockError::Conflict`] (ownership enforced by `X-Client-Id`).
    fn extend(
        &self,
        key: &ResourceKey,
        lock_id: &str,
        holder: &str,
        ttl_ms: u64,
    ) -> Result<(), LockError> {
        let now = self.clock.now();
        let mut map = self.locks.lock().expect("lock registry poisoned");
        match map.get_mut(key) {
            Some(entry) if entry.lock_id == lock_id && entry.expires_at > now => {
                if entry.holder != holder {
                    return Err(LockError::Conflict);
                }
                entry.expires_at = now + Duration::from_millis(ttl_ms);
                Ok(())
            }
            _ => Err(LockError::NotFound),
        }
    }

    /// Release a live lock owned by `holder`.
    ///
    /// Same ownership/identity rules as [`extend`](Self::extend): non-owner →
    /// [`LockError::Conflict`], missing/wrong-id/expired → [`LockError::NotFound`].
    fn release(&self, key: &ResourceKey, lock_id: &str, holder: &str) -> Result<(), LockError> {
        let now = self.clock.now();
        let mut map = self.locks.lock().expect("lock registry poisoned");
        match map.get(key) {
            Some(entry) if entry.lock_id == lock_id && entry.expires_at > now => {
                if entry.holder != holder {
                    return Err(LockError::Conflict);
                }
            }
            _ => return Err(LockError::NotFound),
        }
        map.remove(key);
        drop(map);
        Ok(())
    }
}

/// Format an absolute instant as the contract's RFC3339 `lock_expiration`.
fn format_rfc3339(at: SystemTime) -> String {
    humantime::format_rfc3339_millis(at).to_string()
}

// ---- HTTP surface ----------------------------------------------------------

/// Validate and extract the required `X-Client-Id` holder identity (1–256
/// chars); a missing or out-of-range value is a contract-shaped `400`.
fn client_id(headers: &HeaderMap) -> Result<String, ApiError> {
    match headers.get("x-client-id").and_then(|v| v.to_str().ok()) {
        Some(value) if (1..=256).contains(&value.chars().count()) => Ok(value.to_owned()),
        _ => Err(ApiError::BadRequest(GenericError {
            error_code: "invalid-parameter".to_owned(),
            message: "Missing or invalid X-Client-Id header (1-256 chars required)".to_owned(),
            parameters: BTreeMap::new(),
        })),
    }
}

/// Read the optional `X-Client-Id` on a GET: present and in range yields the
/// holder identity (so `owned` can be computed), absent/invalid yields `None`
/// (the read still succeeds, just with `owned: false`). Reads, unlike
/// mutations, do not require the header.
fn client_id_opt(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-client-id")
        .and_then(|v| v.to_str().ok())
        .filter(|value| (1..=256).contains(&value.chars().count()))
        .map(ToOwned::to_owned)
}

/// Map a [`LockError`] to a contract-shaped [`ApiError`] with resource context.
fn lock_error(error: LockError, kind: EntityKind, id: &str, lock_id: Option<&str>) -> ApiError {
    let mut parameters = BTreeMap::from([
        ("entity".to_owned(), collection_segment(kind).to_owned()),
        ("entity_id".to_owned(), id.to_owned()),
    ]);
    if let Some(lock_id) = lock_id {
        parameters.insert("lock_id".to_owned(), lock_id.to_owned());
    }
    match error {
        LockError::Conflict => ApiError::Conflict(GenericError {
            error_code: "lock-conflict".to_owned(),
            message: "The entity is locked by another client".to_owned(),
            parameters,
        }),
        LockError::NotFound => ApiError::NotFound(GenericError {
            error_code: "lock-not-found".to_owned(),
            message: "Lock not found".to_owned(),
            parameters,
        }),
    }
}

/// The lock CRUD routes for one entity `kind`, mounted under
/// `/{collection}/{id}/locks`. Mirrors the per-kind `kind_routes` pattern.
pub fn lock_routes(kind: EntityKind) -> Router<ServerState> {
    let base = format!("{API_BASE}/{}/{{id}}/locks", collection_segment(kind));
    let item = format!("{base}/{{lock_id}}");
    Router::new()
        .route(
            &base,
            post(
                move |State(state): State<ServerState>,
                      Path(id): Path<String>,
                      headers: HeaderMap,
                      Json(req): Json<AcquireLockRequest>| async move {
                    let holder = client_id(&headers)?;
                    let lock = state
                        .locks()
                        .acquire(
                            (kind, id.clone()),
                            holder,
                            req.lock_expiration,
                            req.break_lock.unwrap_or(false),
                            req.scopes,
                        )
                        .map_err(|e| lock_error(e, kind, &id, None))?;
                    Ok::<_, ApiError>((StatusCode::CREATED, Json(lock)))
                },
            )
            .get(
                // `GET /{entity}/{id}/locks` — list live locks (`REQ_0963`).
                // `X-Client-Id` is optional here; it only decides `owned`.
                move |State(state): State<ServerState>,
                      Path(id): Path<String>,
                      headers: HeaderMap| async move {
                    let requester = client_id_opt(&headers);
                    let items = state.locks().list_for(&(kind, id), requester.as_deref());
                    let total_count = items.len();
                    Json(LockList {
                        items,
                        x_medkit: LockListMeta { total_count },
                    })
                },
            ),
        )
        .route(
            &item,
            axum::routing::get(
                // `GET /{entity}/{id}/locks/{lock_id}` — single lock (`REQ_0963`).
                move |State(state): State<ServerState>,
                      Path((id, lock_id)): Path<(String, String)>,
                      headers: HeaderMap| async move {
                    let requester = client_id_opt(&headers);
                    state
                        .locks()
                        .get_one(&(kind, id.clone()), &lock_id, requester.as_deref())
                        .map(Json)
                        .ok_or_else(|| lock_error(LockError::NotFound, kind, &id, Some(&lock_id)))
                },
            )
            .put(
                move |State(state): State<ServerState>,
                      Path((id, lock_id)): Path<(String, String)>,
                      headers: HeaderMap,
                      Json(req): Json<ExtendLockRequest>| async move {
                    let holder = client_id(&headers)?;
                    state
                        .locks()
                        .extend(&(kind, id.clone()), &lock_id, &holder, req.lock_expiration)
                        .map_err(|e| lock_error(e, kind, &id, Some(&lock_id)))?;
                    Ok::<_, ApiError>(StatusCode::NO_CONTENT)
                },
            )
            .delete(
                move |State(state): State<ServerState>,
                      Path((id, lock_id)): Path<(String, String)>,
                      headers: HeaderMap| async move {
                    let holder = client_id(&headers)?;
                    state
                        .locks()
                        .release(&(kind, id.clone()), &lock_id, &holder)
                        .map_err(|e| lock_error(e, kind, &id, Some(&lock_id)))?;
                    Ok::<_, ApiError>(StatusCode::NO_CONTENT)
                },
            ),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake clock the tests advance by hand, so TTL expiry is deterministic.
    /// The shared `Arc` lets the test keep a handle after the registry takes one.
    #[derive(Clone)]
    struct TestClock(std::sync::Arc<AtomicU64>);

    impl TestClock {
        fn new(millis: u64) -> Self {
            Self(std::sync::Arc::new(AtomicU64::new(millis)))
        }
        fn advance(&self, millis: u64) {
            self.0.fetch_add(millis, Ordering::SeqCst);
        }
    }

    impl Clock for TestClock {
        fn now(&self) -> SystemTime {
            SystemTime::UNIX_EPOCH + Duration::from_millis(self.0.load(Ordering::SeqCst))
        }
    }

    fn key(id: &str) -> ResourceKey {
        (EntityKind::Component, id.to_owned())
    }

    /// `REQ_0940` — acquire returns an owned lock; a second client without
    /// `break_lock` is refused with a conflict.
    #[test]
    fn acquire_then_second_client_conflicts() {
        let reg = LockRegistry::system();
        let lock = reg
            .acquire(key("c1"), "alice".to_owned(), 60_000, false, None)
            .expect("first acquire");
        assert!(lock.owned);
        assert!(lock.id.starts_with("lck-"));

        let conflict = reg.acquire(key("c1"), "bob".to_owned(), 60_000, false, None);
        assert_eq!(conflict.unwrap_err(), LockError::Conflict);

        // A different resource is independently lockable.
        assert!(
            reg.acquire(key("c2"), "bob".to_owned(), 60_000, false, None)
                .is_ok()
        );
    }

    /// `REQ_0942` — `break_lock` evicts a held lock (supervisor override).
    #[test]
    fn break_lock_evicts_incumbent() {
        let reg = LockRegistry::system();
        reg.acquire(key("c1"), "alice".to_owned(), 60_000, false, None)
            .expect("alice acquires");
        let stolen = reg
            .acquire(key("c1"), "bob".to_owned(), 60_000, true, None)
            .expect("break_lock acquires");
        assert!(stolen.owned);
    }

    /// `REQ_0941` — a lock auto-releases once its TTL elapses; the resource is
    /// then freely re-acquirable by anyone.
    #[test]
    fn ttl_expiry_auto_releases() {
        let clock = TestClock::new(0);
        let reg = LockRegistry::with_clock(Box::new(clock.clone()));
        reg.acquire(key("c1"), "alice".to_owned(), 1_000, false, None)
            .expect("alice acquires for 1s");
        // Before expiry, bob is refused.
        assert_eq!(
            reg.acquire(key("c1"), "bob".to_owned(), 1_000, false, None)
                .unwrap_err(),
            LockError::Conflict
        );
        // Advance past the TTL: the lock auto-releases and bob acquires freely.
        clock.advance(1_500);
        assert!(
            reg.acquire(key("c1"), "bob".to_owned(), 1_000, false, None)
                .is_ok()
        );
    }

    /// `REQ_0943` — ownership is enforced by `X-Client-Id`: a non-owner cannot
    /// extend or release; the owner can, and release frees the resource.
    #[test]
    fn ownership_enforced_on_extend_and_release() {
        let reg = LockRegistry::system();
        let lock = reg
            .acquire(key("c1"), "alice".to_owned(), 60_000, false, None)
            .expect("alice acquires");

        // A non-owner cannot extend or release the live lock.
        assert_eq!(
            reg.extend(&key("c1"), &lock.id, "bob", 60_000).unwrap_err(),
            LockError::Conflict
        );
        assert_eq!(
            reg.release(&key("c1"), &lock.id, "bob").unwrap_err(),
            LockError::Conflict
        );

        // A wrong lock id is not found.
        assert_eq!(
            reg.extend(&key("c1"), "lck-999", "alice", 60_000)
                .unwrap_err(),
            LockError::NotFound
        );

        // The owner extends, then releases; release frees the resource.
        reg.extend(&key("c1"), &lock.id, "alice", 60_000)
            .expect("owner extends");
        reg.release(&key("c1"), &lock.id, "alice")
            .expect("owner releases");
        assert!(
            reg.acquire(key("c1"), "bob".to_owned(), 60_000, false, None)
                .is_ok(),
            "resource is free after release"
        );
    }

    /// `REQ_0963` — reads surface the live lock and compute `owned` from the
    /// requester; an expired lock is omitted and a wrong id is `None`.
    #[test]
    fn reads_surface_live_lock_with_owner_view() {
        let clock = TestClock::new(0);
        let reg = LockRegistry::with_clock(Box::new(clock.clone()));
        let lock = reg
            .acquire(
                key("c1"),
                "alice".to_owned(),
                1_000,
                false,
                Some(vec!["s".to_owned()]),
            )
            .expect("acquire");

        // List as the owner -> owned: true, scopes echoed.
        let owned = reg.list_for(&key("c1"), Some("alice"));
        assert_eq!(owned.len(), 1);
        assert!(owned[0].owned);
        assert_eq!(
            owned[0].scopes.as_deref(),
            Some(["s".to_owned()].as_slice())
        );

        // List as a stranger (or anonymous) -> owned: false.
        assert!(!reg.list_for(&key("c1"), Some("bob"))[0].owned);
        assert!(!reg.list_for(&key("c1"), None)[0].owned);

        // Detail by id, then a wrong id -> None.
        assert_eq!(
            reg.get_one(&key("c1"), &lock.id, Some("alice"))
                .map(|l| l.owned),
            Some(true)
        );
        assert!(reg.get_one(&key("c1"), "lck-999", None).is_none());

        // Past the TTL the resource reads as empty (auto-released).
        clock.advance(1_500);
        assert!(reg.list_for(&key("c1"), Some("alice")).is_empty());
        assert!(reg.get_one(&key("c1"), &lock.id, Some("alice")).is_none());
    }

    /// The RFC3339 expiry is the absolute instant `now + ttl`, formatted as a
    /// `Z`-suffixed UTC timestamp (the contract's `lock_expiration` shape).
    #[test]
    fn lock_expiration_is_rfc3339_absolute() {
        let reg = LockRegistry::with_clock(Box::new(TestClock::new(0)));
        let lock = reg
            .acquire(key("c1"), "alice".to_owned(), 1_000, false, None)
            .expect("acquire");
        // epoch + 1000ms = 1970-01-01T00:00:01.000Z
        assert_eq!(lock.lock_expiration, "1970-01-01T00:00:01.000Z");
    }
}
