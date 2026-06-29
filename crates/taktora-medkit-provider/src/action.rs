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

use std::collections::HashMap;
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
}

/// The resource a catalogue/execution is keyed by.
type ResourceKey = (EntityKind, String);

/// One tracked execution plus the resource/op it belongs to (for filtering).
#[derive(Clone, Debug)]
struct ExecRecord {
    key: ResourceKey,
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
}
