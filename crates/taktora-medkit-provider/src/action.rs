//! The **write/action seam** for `taktora-medkit` — the command-side analogue of
//! [`Provider`](crate::Provider) (`REQ_0969`).
//!
//! The diagnostic gateway performs every write (operation executions, config
//! writes, lifecycle transitions, …) through the [`ActionSink`] trait, never
//! touching taktora directly — exactly as it reads through
//! [`Provider`](crate::Provider). This is
//! the facade: the gateway depends only on the trait, so an in-memory
//! [`SimActionSink`] backs tests and the walking skeleton, and a future binding
//! crate backs production.
//!
//! # Safety boundary (deferred)
//!
//! v1 deliberately performs **no real effect**: [`SimActionSink`] tracks
//! executions in memory and touches no safety-critical (SC) resource, so there
//! is nothing to gate yet. When a real-effect binding lands with deep taktora
//! support, the write-surface safety gate (`ADR_0119`) re-enters **at this
//! seam** — a `SafetyGate<RealBindingSink>` decorator wrapping the trait — so no
//! handler changes. Until then the surface is shape-complete and fully testable
//! against the simulation (`ADR_0126`).
//!
//! This crate carries **zero** taktora dependencies, holding the
//! extractable-core invariant (`REQ_0916`, `ADR_0111`).

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use serde_json::Value;
use taktora_medkit_model::EntityKind;

/// The target SOVD resource a write acts on: an entity kind plus its id.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceRef {
    /// The kind of the addressed entity (app, component, function, area).
    pub kind: EntityKind,
    /// The addressed entity's id.
    pub id: String,
}

impl ResourceRef {
    /// A reference to entity `id` of `kind`.
    pub fn new(kind: EntityKind, id: impl Into<String>) -> Self {
        Self {
            kind,
            id: id.into(),
        }
    }
}

/// The lifecycle state of an async action execution, in the contract's
/// lower-case wire vocabulary.
///
/// The in-memory [`SimActionSink`] completes synchronously, so it only emits
/// `Completed`; the full vocabulary is carried here because a real binding (and
/// the wire contract) needs `Pending`/`Running`/`Failed` too.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    /// Accepted, not yet started.
    Pending,
    /// In progress.
    Running,
    /// Finished successfully; `result` carries the outcome.
    Completed,
    /// Finished with an error; `result` carries the detail.
    Failed,
    /// Cancelled before completion.
    Cancelled,
}

/// One operation available on a target — the catalogue entry served under
/// `…/operations`.
#[derive(Clone, Debug, Serialize)]
pub struct OperationDef {
    /// The operation id (URL path component).
    pub id: String,
    /// A human-readable name.
    pub name: String,
    /// An optional description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// A reified execution the registry tracks and the gateway serves under
/// `…/executions/{id}`.
#[derive(Clone, Debug, Serialize)]
pub struct Execution {
    /// The server-assigned execution id.
    pub id: String,
    /// The operation this execution belongs to.
    pub operation_id: String,
    /// The current lifecycle state.
    pub status: ExecutionStatus,
    /// The outcome, once the execution reaches a terminal state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
}

/// One configuration entry on a target — the value stored under
/// `…/configurations/{id}` (`REQ_0971`).
#[derive(Clone, Debug, Serialize)]
pub struct ConfigEntry {
    /// The configuration id (URL path component).
    pub id: String,
    /// The stored configuration value (an arbitrary JSON document).
    pub value: Value,
}

/// One bulk-data category on a target — the entry served under `…/bulk-data`
/// (`REQ_0972`).
#[derive(Clone, Debug, Serialize)]
pub struct BulkCategory {
    /// The category id (URL path component).
    pub id: String,
    /// The number of stored file descriptors in the category.
    pub count: usize,
}

/// One stored bulk-data file descriptor — the entry served under
/// `…/bulk-data/{category_id}/{file_id}` (`REQ_0972`).
#[derive(Clone, Debug, Serialize)]
pub struct BulkDescriptor {
    /// The server-assigned file id (URL path component).
    pub id: String,
    /// The stored payload size in bytes.
    pub size: usize,
}

/// One stored script on a target — the metadata served under `…/scripts` and
/// `…/scripts/{script_id}` (`REQ_0973`).
#[derive(Clone, Debug, Serialize)]
pub struct ScriptDef {
    /// The server-assigned script id (URL path component).
    pub id: String,
    /// The stored script size in bytes.
    pub size: usize,
}

/// One software-update record on the global updates surface — the entry served
/// under `…/updates` and `…/updates/{update_id}` (`REQ_0974`).
///
/// Unlike the other write families, updates are **global** (not per-entity): the
/// contract mounts them at the top level (`/api/v1/updates…`), so an
/// [`UpdateRecord`] carries no [`ResourceRef`].
#[derive(Clone, Debug, Serialize)]
pub struct UpdateRecord {
    /// The server-assigned update id (URL path component).
    pub id: String,
    /// The lifecycle state (`registered` → `prepared` → `executed`).
    pub status: String,
}

/// A write/action failure, mapped to a contract-shaped error by the gateway.
///
/// Reserves no `Forbidden` variant yet: the safety gate is deferred (`ADR_0126`),
/// so v1 never refuses on safety grounds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ActionError {
    /// Unknown target, operation, or execution (`404`).
    NotFound,
    /// An illegal transition for the current state (`409`).
    Conflict,
    /// Malformed arguments (`400`); carries a human-readable reason.
    BadRequest(String),
}

/// The command (write) seam — the write analogue of [`Provider`](crate::Provider).
///
/// The gateway depends only on this trait. Implementations track execution state
/// themselves (the registry lives behind the sink, as the read snapshot lives
/// behind the provider). All methods are synchronous and must be non-blocking,
/// like the read seam: a real binding forwards to an off-control-path queue
/// rather than blocking on the runtime.
pub trait ActionSink: Send + Sync {
    /// The operations available on `target` (empty if none / unknown target).
    fn operations(&self, target: &ResourceRef) -> Vec<OperationDef>;

    /// Start an execution of operation `op` on `target` with `args`.
    ///
    /// # Errors
    ///
    /// [`ActionError::NotFound`] if `op` is not in the target's catalogue;
    /// [`ActionError::BadRequest`] if `args` are rejected.
    fn start_operation(
        &self,
        target: &ResourceRef,
        op: &str,
        args: Value,
    ) -> Result<Execution, ActionError>;

    /// The executions of operation `op` on `target`.
    ///
    /// # Errors
    ///
    /// [`ActionError::NotFound`] if `op` is not in the target's catalogue.
    fn executions(&self, target: &ResourceRef, op: &str) -> Result<Vec<Execution>, ActionError>;

    /// One execution by id.
    ///
    /// # Errors
    ///
    /// [`ActionError::NotFound`] if no such execution exists for `op` on `target`.
    fn execution(
        &self,
        target: &ResourceRef,
        op: &str,
        exec_id: &str,
    ) -> Result<Execution, ActionError>;

    /// Cancel (if running) or remove (if terminal) an execution.
    ///
    /// Idempotent cancel-or-remove: either way the execution is gone afterwards.
    ///
    /// # Errors
    ///
    /// [`ActionError::NotFound`] if no such execution exists for `op` on `target`.
    fn cancel_execution(
        &self,
        target: &ResourceRef,
        op: &str,
        exec_id: &str,
    ) -> Result<(), ActionError>;

    /// The configurations currently set on `target` (empty if none), ordered by
    /// id (`REQ_0971`).
    fn configurations(&self, target: &ResourceRef) -> Vec<ConfigEntry>;

    /// One configuration by id (`REQ_0971`).
    ///
    /// # Errors
    ///
    /// [`ActionError::NotFound`] if `config_id` is not set on `target`.
    fn configuration(
        &self,
        target: &ResourceRef,
        config_id: &str,
    ) -> Result<ConfigEntry, ActionError>;

    /// Upsert configuration `config_id` on `target` to `value` (`REQ_0971`).
    ///
    /// Idempotent set: creating or overwriting either way yields the stored
    /// entry.
    ///
    /// # Errors
    ///
    /// [`ActionError::BadRequest`] if `value` is rejected by a real binding; the
    /// in-memory simulation always succeeds.
    fn set_configuration(
        &self,
        target: &ResourceRef,
        config_id: &str,
        value: Value,
    ) -> Result<ConfigEntry, ActionError>;

    /// Delete configuration `config_id` from `target` (`REQ_0971`).
    ///
    /// # Errors
    ///
    /// [`ActionError::NotFound`] if `config_id` is not set on `target`.
    fn delete_configuration(
        &self,
        target: &ResourceRef,
        config_id: &str,
    ) -> Result<(), ActionError>;

    /// Delete every configuration set on `target` (`REQ_0971`).
    ///
    /// Idempotent clear: succeeds whether or not the target had any.
    ///
    /// # Errors
    ///
    /// [`ActionError::BadRequest`] if a real binding rejects the clear; the
    /// in-memory simulation always succeeds.
    fn delete_configurations(&self, target: &ResourceRef) -> Result<(), ActionError>;

    /// The bulk-data categories on `target` that hold at least one file, each
    /// with its file count (empty if none) — `REQ_0972`.
    fn bulk_categories(&self, target: &ResourceRef) -> Vec<BulkCategory>;

    /// The file descriptors stored in `category` on `target` (empty if the
    /// category is unknown or holds none) — `REQ_0972`.
    fn bulk_descriptors(&self, target: &ResourceRef, category: &str) -> Vec<BulkDescriptor>;

    /// Upload `content` into `category` on `target`, returning the descriptor of
    /// the newly stored file (`REQ_0972`).
    ///
    /// # Errors
    ///
    /// [`ActionError::BadRequest`] if a real binding rejects the upload; the
    /// in-memory simulation always succeeds.
    fn upload_bulk(
        &self,
        target: &ResourceRef,
        category: &str,
        content: Vec<u8>,
    ) -> Result<BulkDescriptor, ActionError>;

    /// Download the stored bytes of `file_id` in `category` on `target`
    /// (`REQ_0972`).
    ///
    /// # Errors
    ///
    /// [`ActionError::NotFound`] if the category or file is unknown.
    fn download_bulk(
        &self,
        target: &ResourceRef,
        category: &str,
        file_id: &str,
    ) -> Result<Vec<u8>, ActionError>;

    /// Delete `file_id` from `category` on `target` (`REQ_0972`).
    ///
    /// # Errors
    ///
    /// [`ActionError::NotFound`] if the category or file is unknown.
    fn delete_bulk(
        &self,
        target: &ResourceRef,
        category: &str,
        file_id: &str,
    ) -> Result<(), ActionError>;

    /// The scripts stored on `target`, ordered by id (empty if none) —
    /// `REQ_0973`.
    fn scripts(&self, target: &ResourceRef) -> Vec<ScriptDef>;

    /// Upload `content` as a new script on `target`, returning its metadata
    /// (`REQ_0973`).
    ///
    /// # Errors
    ///
    /// [`ActionError::BadRequest`] if a real binding rejects the upload; the
    /// in-memory simulation always succeeds.
    fn upload_script(
        &self,
        target: &ResourceRef,
        content: Vec<u8>,
    ) -> Result<ScriptDef, ActionError>;

    /// One script by id (`REQ_0973`).
    ///
    /// # Errors
    ///
    /// [`ActionError::NotFound`] if `script_id` is not stored on `target`.
    fn script(&self, target: &ResourceRef, script_id: &str) -> Result<ScriptDef, ActionError>;

    /// Delete `script_id` from `target` (`REQ_0973`).
    ///
    /// # Errors
    ///
    /// [`ActionError::NotFound`] if `script_id` is not stored on `target`.
    fn delete_script(&self, target: &ResourceRef, script_id: &str) -> Result<(), ActionError>;

    /// Start an execution of `script_id` on `target` (`REQ_0973`).
    ///
    /// Reuses the operations [`Execution`] type: the simulation completes
    /// synchronously with a result echoing the script id.
    ///
    /// # Errors
    ///
    /// [`ActionError::NotFound`] if `script_id` is not stored on `target`.
    fn start_script(&self, target: &ResourceRef, script_id: &str)
    -> Result<Execution, ActionError>;

    /// One script execution by id (`REQ_0973`).
    ///
    /// # Errors
    ///
    /// [`ActionError::NotFound`] if no such execution exists for `script_id` on
    /// `target`.
    fn script_execution(
        &self,
        target: &ResourceRef,
        script_id: &str,
        exec_id: &str,
    ) -> Result<Execution, ActionError>;

    /// Cancel (if running) or remove (if terminal) a script execution
    /// (`REQ_0973`).
    ///
    /// Idempotent cancel-or-remove: either way the execution is gone afterwards.
    ///
    /// # Errors
    ///
    /// [`ActionError::NotFound`] if no such execution exists for `script_id` on
    /// `target`.
    fn cancel_script_execution(
        &self,
        target: &ResourceRef,
        script_id: &str,
        exec_id: &str,
    ) -> Result<(), ActionError>;

    /// The software-update records currently registered, ordered by id (empty if
    /// none) — `REQ_0974`.
    ///
    /// Updates are a **global** family: this method takes no [`ResourceRef`].
    fn updates(&self) -> Vec<UpdateRecord>;

    /// Register a new software update from `spec` (the request body), returning
    /// its freshly-assigned record in the `registered` state (`REQ_0974`).
    ///
    /// `spec` is the opaque update descriptor; the in-memory simulation may ignore
    /// or echo it. Always succeeds.
    fn register_update(&self, spec: Value) -> UpdateRecord;

    /// One software-update record by id (`REQ_0974`).
    ///
    /// # Errors
    ///
    /// [`ActionError::NotFound`] if `update_id` is not registered.
    fn update(&self, update_id: &str) -> Result<UpdateRecord, ActionError>;

    /// Transition update `update_id` to the `prepared` state (`REQ_0974`).
    ///
    /// # Errors
    ///
    /// [`ActionError::NotFound`] if `update_id` is not registered.
    fn prepare_update(&self, update_id: &str) -> Result<UpdateRecord, ActionError>;

    /// Transition update `update_id` to the `executed` state (`REQ_0974`).
    ///
    /// # Errors
    ///
    /// [`ActionError::NotFound`] if `update_id` is not registered.
    fn execute_update(&self, update_id: &str) -> Result<UpdateRecord, ActionError>;

    /// Drive update `update_id` through automated execution to the `executed`
    /// state (`REQ_0974`).
    ///
    /// The simulation treats automated execution as a synchronous transition to
    /// `executed`, like [`execute_update`](Self::execute_update).
    ///
    /// # Errors
    ///
    /// [`ActionError::NotFound`] if `update_id` is not registered.
    fn automated_update(&self, update_id: &str) -> Result<UpdateRecord, ActionError>;

    /// Delete update `update_id` (`REQ_0974`).
    ///
    /// # Errors
    ///
    /// [`ActionError::NotFound`] if `update_id` is not registered.
    fn delete_update(&self, update_id: &str) -> Result<(), ActionError>;
}

/// The resource a catalogue/execution is keyed by.
type ResourceKey = (EntityKind, String);

/// The per-resource bulk-data store: category → file id → stored bytes.
type BulkStore = HashMap<ResourceKey, BTreeMap<String, BTreeMap<String, Vec<u8>>>>;

/// One tracked execution plus the resource/op it belongs to (for filtering).
#[derive(Clone, Debug)]
struct ExecRecord {
    key: ResourceKey,
    execution: Execution,
}

/// One tracked script execution plus the resource/script it belongs to (for
/// filtering), kept separate from operation executions so the two surfaces never
/// collide on exec id.
#[derive(Clone, Debug)]
struct ScriptExecRecord {
    key: ResourceKey,
    script_id: String,
    execution: Execution,
}

/// An in-memory [`ActionSink`] for tests and the walking skeleton (`REQ_0969`).
///
/// Operations are configured per resource via [`with_operation`](Self::with_operation),
/// like [`MockProvider`](crate::MockProvider)'s builders. A started execution
/// **completes synchronously** with a result that echoes the request args — the
/// simulation performs no real effect, so it touches no SC resource. A real
/// binding (deferred) would surface `Pending`/`Running` and drive transitions
/// off the control path.
#[derive(Default)]
pub struct SimActionSink {
    catalogue: Mutex<HashMap<ResourceKey, Vec<OperationDef>>>,
    executions: Mutex<HashMap<String, ExecRecord>>,
    configs: Mutex<HashMap<ResourceKey, BTreeMap<String, Value>>>,
    /// Per-resource bulk-data store: category → file id → stored bytes.
    bulk: Mutex<BulkStore>,
    /// Per-resource script store: script id → stored size in bytes.
    scripts: Mutex<HashMap<ResourceKey, BTreeMap<String, usize>>>,
    /// Script executions keyed by exec id (separate from operation executions).
    script_execs: Mutex<HashMap<String, ScriptExecRecord>>,
    /// The global software-update store, keyed by update id (`REQ_0974`).
    updates: Mutex<BTreeMap<String, UpdateRecord>>,
    next: AtomicU64,
}

impl SimActionSink {
    /// An empty sink (no operations on any resource).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register operation `op` (id == name) as available on entity `id` of `kind`.
    ///
    /// # Panics
    ///
    /// Panics if the catalogue mutex is poisoned (a prior holder panicked).
    #[must_use]
    pub fn with_operation(
        self,
        kind: EntityKind,
        id: impl Into<String>,
        op: impl Into<String>,
    ) -> Self {
        let op = op.into();
        let def = OperationDef {
            id: op.clone(),
            name: op,
            description: None,
        };
        self.catalogue
            .lock()
            .expect("action catalogue poisoned")
            .entry((kind, id.into()))
            .or_default()
            .push(def);
        self
    }

    fn has_operation(&self, target: &ResourceRef, op: &str) -> bool {
        self.catalogue
            .lock()
            .expect("action catalogue poisoned")
            .get(&(target.kind, target.id.clone()))
            .is_some_and(|ops| ops.iter().any(|d| d.id == op))
    }

    fn has_script(&self, target: &ResourceRef, script_id: &str) -> bool {
        self.scripts
            .lock()
            .expect("script store poisoned")
            .get(&(target.kind, target.id.clone()))
            .is_some_and(|m| m.contains_key(script_id))
    }

    /// Set update `update_id`'s status to `status`, returning the updated record;
    /// [`ActionError::NotFound`] if it is not registered (`REQ_0974`).
    fn transition_update(
        &self,
        update_id: &str,
        status: &str,
    ) -> Result<UpdateRecord, ActionError> {
        let mut map = self.updates.lock().expect("update store poisoned");
        let Some(record) = map.get_mut(update_id) else {
            return Err(ActionError::NotFound);
        };
        status.clone_into(&mut record.status);
        let updated = record.clone();
        drop(map);
        Ok(updated)
    }
}

impl ActionSink for SimActionSink {
    fn operations(&self, target: &ResourceRef) -> Vec<OperationDef> {
        self.catalogue
            .lock()
            .expect("action catalogue poisoned")
            .get(&(target.kind, target.id.clone()))
            .cloned()
            .unwrap_or_default()
    }

    fn start_operation(
        &self,
        target: &ResourceRef,
        op: &str,
        args: Value,
    ) -> Result<Execution, ActionError> {
        if !self.has_operation(target, op) {
            return Err(ActionError::NotFound);
        }
        let id = format!("exec-{}", self.next.fetch_add(1, Ordering::SeqCst));
        // The simulation completes synchronously: a real binding would return
        // `Pending` and drive the transition off the control path.
        let execution = Execution {
            id: id.clone(),
            operation_id: op.to_owned(),
            status: ExecutionStatus::Completed,
            result: Some(serde_json::json!({ "echo": args })),
        };
        self.executions
            .lock()
            .expect("execution registry poisoned")
            .insert(
                id,
                ExecRecord {
                    key: (target.kind, target.id.clone()),
                    execution: execution.clone(),
                },
            );
        Ok(execution)
    }

    fn executions(&self, target: &ResourceRef, op: &str) -> Result<Vec<Execution>, ActionError> {
        if !self.has_operation(target, op) {
            return Err(ActionError::NotFound);
        }
        let key = (target.kind, target.id.clone());
        let mut items: Vec<Execution> = self
            .executions
            .lock()
            .expect("execution registry poisoned")
            .values()
            .filter(|r| r.key == key && r.execution.operation_id == op)
            .map(|r| r.execution.clone())
            .collect();
        items.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(items)
    }

    fn execution(
        &self,
        target: &ResourceRef,
        op: &str,
        exec_id: &str,
    ) -> Result<Execution, ActionError> {
        let key = (target.kind, target.id.clone());
        self.executions
            .lock()
            .expect("execution registry poisoned")
            .get(exec_id)
            .filter(|r| r.key == key && r.execution.operation_id == op)
            .map(|r| r.execution.clone())
            .ok_or(ActionError::NotFound)
    }

    fn cancel_execution(
        &self,
        target: &ResourceRef,
        op: &str,
        exec_id: &str,
    ) -> Result<(), ActionError> {
        let key = (target.kind, target.id.clone());
        let mut map = self.executions.lock().expect("execution registry poisoned");
        match map.get(exec_id) {
            Some(r) if r.key == key && r.execution.operation_id == op => {}
            _ => return Err(ActionError::NotFound),
        }
        map.remove(exec_id);
        drop(map);
        Ok(())
    }

    fn configurations(&self, target: &ResourceRef) -> Vec<ConfigEntry> {
        self.configs
            .lock()
            .expect("config store poisoned")
            .get(&(target.kind, target.id.clone()))
            .map(|m| {
                m.iter()
                    .map(|(id, value)| ConfigEntry {
                        id: id.clone(),
                        value: value.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn configuration(
        &self,
        target: &ResourceRef,
        config_id: &str,
    ) -> Result<ConfigEntry, ActionError> {
        self.configs
            .lock()
            .expect("config store poisoned")
            .get(&(target.kind, target.id.clone()))
            .and_then(|m| m.get(config_id))
            .map(|value| ConfigEntry {
                id: config_id.to_owned(),
                value: value.clone(),
            })
            .ok_or(ActionError::NotFound)
    }

    fn set_configuration(
        &self,
        target: &ResourceRef,
        config_id: &str,
        value: Value,
    ) -> Result<ConfigEntry, ActionError> {
        let mut map = self.configs.lock().expect("config store poisoned");
        map.entry((target.kind, target.id.clone()))
            .or_default()
            .insert(config_id.to_owned(), value.clone());
        drop(map);
        Ok(ConfigEntry {
            id: config_id.to_owned(),
            value,
        })
    }

    fn delete_configuration(
        &self,
        target: &ResourceRef,
        config_id: &str,
    ) -> Result<(), ActionError> {
        let key = (target.kind, target.id.clone());
        let mut map = self.configs.lock().expect("config store poisoned");
        let removed = map
            .get_mut(&key)
            .is_some_and(|m| m.remove(config_id).is_some());
        if !removed {
            return Err(ActionError::NotFound);
        }
        drop(map);
        Ok(())
    }

    fn delete_configurations(&self, target: &ResourceRef) -> Result<(), ActionError> {
        let mut map = self.configs.lock().expect("config store poisoned");
        map.remove(&(target.kind, target.id.clone()));
        drop(map);
        Ok(())
    }

    fn bulk_categories(&self, target: &ResourceRef) -> Vec<BulkCategory> {
        self.bulk
            .lock()
            .expect("bulk store poisoned")
            .get(&(target.kind, target.id.clone()))
            .map(|cats| {
                cats.iter()
                    .filter(|(_, files)| !files.is_empty())
                    .map(|(id, files)| BulkCategory {
                        id: id.clone(),
                        count: files.len(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn bulk_descriptors(&self, target: &ResourceRef, category: &str) -> Vec<BulkDescriptor> {
        self.bulk
            .lock()
            .expect("bulk store poisoned")
            .get(&(target.kind, target.id.clone()))
            .and_then(|cats| cats.get(category))
            .map(|files| {
                files
                    .iter()
                    .map(|(id, bytes)| BulkDescriptor {
                        id: id.clone(),
                        size: bytes.len(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn upload_bulk(
        &self,
        target: &ResourceRef,
        category: &str,
        content: Vec<u8>,
    ) -> Result<BulkDescriptor, ActionError> {
        let id = format!("file-{}", self.next.fetch_add(1, Ordering::SeqCst));
        let size = content.len();
        let mut map = self.bulk.lock().expect("bulk store poisoned");
        map.entry((target.kind, target.id.clone()))
            .or_default()
            .entry(category.to_owned())
            .or_default()
            .insert(id.clone(), content);
        drop(map);
        Ok(BulkDescriptor { id, size })
    }

    fn download_bulk(
        &self,
        target: &ResourceRef,
        category: &str,
        file_id: &str,
    ) -> Result<Vec<u8>, ActionError> {
        self.bulk
            .lock()
            .expect("bulk store poisoned")
            .get(&(target.kind, target.id.clone()))
            .and_then(|cats| cats.get(category))
            .and_then(|files| files.get(file_id))
            .cloned()
            .ok_or(ActionError::NotFound)
    }

    fn delete_bulk(
        &self,
        target: &ResourceRef,
        category: &str,
        file_id: &str,
    ) -> Result<(), ActionError> {
        let key = (target.kind, target.id.clone());
        let mut map = self.bulk.lock().expect("bulk store poisoned");
        let removed = map
            .get_mut(&key)
            .and_then(|cats| cats.get_mut(category))
            .is_some_and(|files| files.remove(file_id).is_some());
        if !removed {
            return Err(ActionError::NotFound);
        }
        drop(map);
        Ok(())
    }

    fn scripts(&self, target: &ResourceRef) -> Vec<ScriptDef> {
        self.scripts
            .lock()
            .expect("script store poisoned")
            .get(&(target.kind, target.id.clone()))
            .map(|m| {
                m.iter()
                    .map(|(id, &size)| ScriptDef {
                        id: id.clone(),
                        size,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn upload_script(
        &self,
        target: &ResourceRef,
        content: Vec<u8>,
    ) -> Result<ScriptDef, ActionError> {
        let id = format!("script-{}", self.next.fetch_add(1, Ordering::SeqCst));
        let size = content.len();
        let mut map = self.scripts.lock().expect("script store poisoned");
        map.entry((target.kind, target.id.clone()))
            .or_default()
            .insert(id.clone(), size);
        drop(map);
        Ok(ScriptDef { id, size })
    }

    fn script(&self, target: &ResourceRef, script_id: &str) -> Result<ScriptDef, ActionError> {
        self.scripts
            .lock()
            .expect("script store poisoned")
            .get(&(target.kind, target.id.clone()))
            .and_then(|m| m.get(script_id))
            .map(|&size| ScriptDef {
                id: script_id.to_owned(),
                size,
            })
            .ok_or(ActionError::NotFound)
    }

    fn delete_script(&self, target: &ResourceRef, script_id: &str) -> Result<(), ActionError> {
        let key = (target.kind, target.id.clone());
        let mut map = self.scripts.lock().expect("script store poisoned");
        let removed = map
            .get_mut(&key)
            .is_some_and(|m| m.remove(script_id).is_some());
        if !removed {
            return Err(ActionError::NotFound);
        }
        drop(map);
        Ok(())
    }

    fn start_script(
        &self,
        target: &ResourceRef,
        script_id: &str,
    ) -> Result<Execution, ActionError> {
        if !self.has_script(target, script_id) {
            return Err(ActionError::NotFound);
        }
        let id = format!("sexec-{}", self.next.fetch_add(1, Ordering::SeqCst));
        // The simulation completes synchronously, like `start_operation`: a real
        // binding would return `Pending` and drive the transition off the path.
        let execution = Execution {
            id: id.clone(),
            operation_id: script_id.to_owned(),
            status: ExecutionStatus::Completed,
            result: Some(serde_json::json!({ "script": script_id })),
        };
        self.script_execs
            .lock()
            .expect("script execution registry poisoned")
            .insert(
                id,
                ScriptExecRecord {
                    key: (target.kind, target.id.clone()),
                    script_id: script_id.to_owned(),
                    execution: execution.clone(),
                },
            );
        Ok(execution)
    }

    fn script_execution(
        &self,
        target: &ResourceRef,
        script_id: &str,
        exec_id: &str,
    ) -> Result<Execution, ActionError> {
        let key = (target.kind, target.id.clone());
        self.script_execs
            .lock()
            .expect("script execution registry poisoned")
            .get(exec_id)
            .filter(|r| r.key == key && r.script_id == script_id)
            .map(|r| r.execution.clone())
            .ok_or(ActionError::NotFound)
    }

    fn cancel_script_execution(
        &self,
        target: &ResourceRef,
        script_id: &str,
        exec_id: &str,
    ) -> Result<(), ActionError> {
        let key = (target.kind, target.id.clone());
        let mut map = self
            .script_execs
            .lock()
            .expect("script execution registry poisoned");
        match map.get(exec_id) {
            Some(r) if r.key == key && r.script_id == script_id => {}
            _ => return Err(ActionError::NotFound),
        }
        map.remove(exec_id);
        drop(map);
        Ok(())
    }

    fn updates(&self) -> Vec<UpdateRecord> {
        self.updates
            .lock()
            .expect("update store poisoned")
            .values()
            .cloned()
            .collect()
    }

    fn register_update(&self, _spec: Value) -> UpdateRecord {
        let id = format!("update-{}", self.next.fetch_add(1, Ordering::SeqCst));
        let record = UpdateRecord {
            id: id.clone(),
            status: "registered".to_owned(),
        };
        self.updates
            .lock()
            .expect("update store poisoned")
            .insert(id, record.clone());
        record
    }

    fn update(&self, update_id: &str) -> Result<UpdateRecord, ActionError> {
        self.updates
            .lock()
            .expect("update store poisoned")
            .get(update_id)
            .cloned()
            .ok_or(ActionError::NotFound)
    }

    fn prepare_update(&self, update_id: &str) -> Result<UpdateRecord, ActionError> {
        self.transition_update(update_id, "prepared")
    }

    fn execute_update(&self, update_id: &str) -> Result<UpdateRecord, ActionError> {
        self.transition_update(update_id, "executed")
    }

    fn automated_update(&self, update_id: &str) -> Result<UpdateRecord, ActionError> {
        self.transition_update(update_id, "executed")
    }

    fn delete_update(&self, update_id: &str) -> Result<(), ActionError> {
        let mut map = self.updates.lock().expect("update store poisoned");
        let removed = map.remove(update_id).is_some();
        drop(map);
        if removed {
            Ok(())
        } else {
            Err(ActionError::NotFound)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> ResourceRef {
        ResourceRef::new(EntityKind::App, "gw")
    }

    /// `REQ_0969` — the sink lists configured operations and refuses unknown ones.
    #[test]
    fn catalogue_gates_start() {
        let sink = SimActionSink::new().with_operation(EntityKind::App, "gw", "reset");
        assert_eq!(sink.operations(&target()).len(), 1);
        assert!(
            sink.start_operation(&target(), "reset", Value::Null)
                .is_ok()
        );
        assert_eq!(
            sink.start_operation(&target(), "unknown", Value::Null)
                .unwrap_err(),
            ActionError::NotFound
        );
    }

    /// `REQ_0969` — start → completed execution; list/get find it; cancel removes
    /// it; a subsequent get is `NotFound`.
    #[test]
    fn execution_lifecycle() {
        let sink = SimActionSink::new().with_operation(EntityKind::App, "gw", "reset");
        let started = sink
            .start_operation(&target(), "reset", serde_json::json!({"force": true}))
            .expect("start");
        assert_eq!(started.status, ExecutionStatus::Completed);
        assert_eq!(
            started.result,
            Some(serde_json::json!({"echo": {"force": true}}))
        );

        assert_eq!(sink.executions(&target(), "reset").unwrap().len(), 1);
        assert!(sink.execution(&target(), "reset", &started.id).is_ok());

        // A different op never sees this execution.
        assert_eq!(
            sink.execution(&target(), "other", &started.id).unwrap_err(),
            ActionError::NotFound
        );

        sink.cancel_execution(&target(), "reset", &started.id)
            .expect("cancel");
        assert_eq!(
            sink.execution(&target(), "reset", &started.id).unwrap_err(),
            ActionError::NotFound
        );
        assert_eq!(
            sink.cancel_execution(&target(), "reset", &started.id)
                .unwrap_err(),
            ActionError::NotFound
        );
    }

    /// `REQ_0971` — upsert → get → overwrite → delete → 404, plus delete-all.
    #[test]
    fn config_upsert_get_delete() {
        let sink = SimActionSink::new();

        // Unset config is NotFound; the list is empty.
        assert_eq!(
            sink.configuration(&target(), "rate").unwrap_err(),
            ActionError::NotFound
        );
        assert_eq!(sink.configurations(&target()).len(), 0);

        // Upsert stores and echoes the value.
        let stored = sink
            .set_configuration(&target(), "rate", serde_json::json!({"hz": 50}))
            .expect("set");
        assert_eq!(stored.id, "rate");
        assert_eq!(stored.value, serde_json::json!({"hz": 50}));

        // Get finds it; the list shows it.
        assert_eq!(
            sink.configuration(&target(), "rate").unwrap().value,
            serde_json::json!({"hz": 50})
        );
        assert_eq!(sink.configurations(&target()).len(), 1);

        // Upsert again overwrites the value.
        sink.set_configuration(&target(), "rate", serde_json::json!({"hz": 100}))
            .expect("update");
        assert_eq!(
            sink.configuration(&target(), "rate").unwrap().value,
            serde_json::json!({"hz": 100})
        );
        assert_eq!(sink.configurations(&target()).len(), 1);

        // Delete one removes it; a second delete is NotFound.
        sink.delete_configuration(&target(), "rate")
            .expect("delete");
        assert_eq!(
            sink.configuration(&target(), "rate").unwrap_err(),
            ActionError::NotFound
        );
        assert_eq!(
            sink.delete_configuration(&target(), "rate").unwrap_err(),
            ActionError::NotFound
        );
    }

    /// `REQ_0971` — delete-all clears every config and always succeeds.
    #[test]
    fn config_delete_all() {
        let sink = SimActionSink::new();
        sink.set_configuration(&target(), "a", serde_json::json!(1))
            .expect("set a");
        sink.set_configuration(&target(), "b", serde_json::json!(2))
            .expect("set b");
        assert_eq!(sink.configurations(&target()).len(), 2);

        sink.delete_configurations(&target()).expect("clear");
        assert_eq!(sink.configurations(&target()).len(), 0);

        // Idempotent: clearing an already-empty resource still succeeds.
        sink.delete_configurations(&target()).expect("clear again");
    }

    /// `REQ_0972` — upload → list/download round-trip → delete → 404.
    #[test]
    fn bulk_upload_download_delete() {
        let sink = SimActionSink::new();

        // No categories and no descriptors on a fresh resource; an unknown file
        // is NotFound.
        assert_eq!(sink.bulk_categories(&target()).len(), 0);
        assert_eq!(sink.bulk_descriptors(&target(), "logs").len(), 0);
        assert_eq!(
            sink.download_bulk(&target(), "logs", "file-1").unwrap_err(),
            ActionError::NotFound
        );

        // Upload stores the bytes and returns a descriptor with the size.
        let payload = b"hello bulk".to_vec();
        let desc = sink
            .upload_bulk(&target(), "logs", payload.clone())
            .expect("upload");
        assert_eq!(desc.size, payload.len());
        assert!(desc.id.starts_with("file-"));

        // The category lists with a count of 1; the descriptor shows up.
        let cats = sink.bulk_categories(&target());
        assert_eq!(cats.len(), 1);
        assert_eq!(cats[0].id, "logs");
        assert_eq!(cats[0].count, 1);
        let descs = sink.bulk_descriptors(&target(), "logs");
        assert_eq!(descs.len(), 1);
        assert_eq!(descs[0].id, desc.id);

        // Download round-trips the exact bytes.
        assert_eq!(
            sink.download_bulk(&target(), "logs", &desc.id).unwrap(),
            payload
        );

        // Delete removes it; a second delete and a download are NotFound, and the
        // (now empty) category drops out of the listing.
        sink.delete_bulk(&target(), "logs", &desc.id)
            .expect("delete");
        assert_eq!(
            sink.delete_bulk(&target(), "logs", &desc.id).unwrap_err(),
            ActionError::NotFound
        );
        assert_eq!(
            sink.download_bulk(&target(), "logs", &desc.id).unwrap_err(),
            ActionError::NotFound
        );
        assert_eq!(sink.bulk_categories(&target()).len(), 0);
    }

    /// `REQ_0973` — upload → get/list → start execution → poll → cancel → 404,
    /// then delete the script → 404; an unknown script never starts.
    #[test]
    fn script_upload_and_execute() {
        let sink = SimActionSink::new();

        // A fresh resource has no scripts; an unknown script is NotFound and
        // never starts an execution.
        assert_eq!(sink.scripts(&target()).len(), 0);
        assert_eq!(
            sink.script(&target(), "script-1").unwrap_err(),
            ActionError::NotFound
        );
        assert_eq!(
            sink.start_script(&target(), "script-1").unwrap_err(),
            ActionError::NotFound
        );

        // Upload stores the bytes and returns metadata with the size.
        let payload = b"#!/bin/sh\necho hi".to_vec();
        let script = sink
            .upload_script(&target(), payload.clone())
            .expect("upload");
        assert_eq!(script.size, payload.len());
        assert!(script.id.starts_with("script-"));

        // Get finds it; the list shows it.
        assert_eq!(
            sink.script(&target(), &script.id).unwrap().size,
            payload.len()
        );
        assert_eq!(sink.scripts(&target()).len(), 1);

        // Start an execution -> synchronously completed, echoing the script id.
        let started = sink.start_script(&target(), &script.id).expect("start");
        assert_eq!(started.status, ExecutionStatus::Completed);
        assert!(started.id.starts_with("sexec-"));
        assert_eq!(
            started.result,
            Some(serde_json::json!({"script": script.id}))
        );

        // Poll finds it; a different script never sees this execution.
        assert!(
            sink.script_execution(&target(), &script.id, &started.id)
                .is_ok()
        );
        assert_eq!(
            sink.script_execution(&target(), "script-other", &started.id)
                .unwrap_err(),
            ActionError::NotFound
        );

        // Cancel removes it; a subsequent poll/cancel is NotFound.
        sink.cancel_script_execution(&target(), &script.id, &started.id)
            .expect("cancel");
        assert_eq!(
            sink.script_execution(&target(), &script.id, &started.id)
                .unwrap_err(),
            ActionError::NotFound
        );
        assert_eq!(
            sink.cancel_script_execution(&target(), &script.id, &started.id)
                .unwrap_err(),
            ActionError::NotFound
        );

        // Delete the script; a second delete and a get are NotFound.
        sink.delete_script(&target(), &script.id).expect("delete");
        assert_eq!(
            sink.delete_script(&target(), &script.id).unwrap_err(),
            ActionError::NotFound
        );
        assert_eq!(
            sink.script(&target(), &script.id).unwrap_err(),
            ActionError::NotFound
        );
        assert_eq!(sink.scripts(&target()).len(), 0);
    }

    /// `REQ_0974` — register → prepare → execute drives the status; the record
    /// lists and reads back; delete removes it and a subsequent read/transition is
    /// `NotFound`. Updates are global (no `ResourceRef`).
    #[test]
    fn update_register_prepare_execute() {
        let sink = SimActionSink::new();

        // A fresh sink has no updates; an unknown id is NotFound on every verb.
        assert_eq!(sink.updates().len(), 0);
        assert_eq!(sink.update("update-1").unwrap_err(), ActionError::NotFound);
        assert_eq!(
            sink.prepare_update("update-1").unwrap_err(),
            ActionError::NotFound
        );

        // Register starts in the `registered` state and lists/reads back.
        let record = sink.register_update(serde_json::json!({"package": "fw-1.2.3"}));
        assert!(record.id.starts_with("update-"));
        assert_eq!(record.status, "registered");
        assert_eq!(sink.updates().len(), 1);
        assert_eq!(sink.update(&record.id).unwrap().status, "registered");

        // Prepare → `prepared`; execute → `executed`.
        assert_eq!(sink.prepare_update(&record.id).unwrap().status, "prepared");
        assert_eq!(sink.update(&record.id).unwrap().status, "prepared");
        assert_eq!(sink.execute_update(&record.id).unwrap().status, "executed");
        assert_eq!(sink.update(&record.id).unwrap().status, "executed");

        // Automated execution also lands in `executed`.
        let auto = sink.register_update(Value::Null);
        assert_eq!(sink.automated_update(&auto.id).unwrap().status, "executed");

        // Delete removes it; a second delete and a read/transition are NotFound.
        sink.delete_update(&record.id).expect("delete");
        assert_eq!(
            sink.delete_update(&record.id).unwrap_err(),
            ActionError::NotFound
        );
        assert_eq!(sink.update(&record.id).unwrap_err(), ActionError::NotFound);
    }
}
