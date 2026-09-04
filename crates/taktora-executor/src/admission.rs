//! Cold-start admission-gate primitives (`AFSR_0005`).

use crate::IntegrityLevel;

/// Boxed cold-start admission check: invoked once before the executor enters
/// `RUNNING` to verify the spatial-isolation context (`AFSR_0005`).
pub type AdmissionCheckFn = Box<dyn Fn(&AdmissionContext) -> AdmissionOutcome + Send + Sync>;

/// Reason for admission rejection.
///
/// Returned by a failed [`AdmissionContext::verify_isolation`] call or
/// user-supplied admission check. Captured by the executor and surfaced via
/// [`crate::ExecutorError::AdmissionRejected`].
#[derive(Clone, Debug)]
pub struct AdmissionFault {
    /// Human-readable reason for the rejection.
    pub reason: String,
}

impl AdmissionFault {
    /// Create a new admission fault with the given reason.
    #[must_use]
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

/// Result of an admission check.
///
/// Returned by the closure configured via
/// [`crate::ExecutorBuilder::admission_check`]. A `Rejected` outcome prevents
/// the executor from entering `RUNNING` — no tasks dispatch, and
/// [`crate::Executor::run`] (and its `run_for` / `run_n` / `run_until`
/// variants) returns [`crate::ExecutorError::AdmissionRejected`].
#[derive(Debug)]
pub enum AdmissionOutcome {
    /// Verification passed; the executor may proceed to `RUNNING`.
    Admitted,
    /// Verification failed; the executor refuses admission and returns the
    /// enclosed fault.
    Rejected(AdmissionFault),
}

/// Context passed to the admission check closure.
///
/// Provides read-only access to executor-observable facts so the integrator's
/// admission logic can inspect the execution environment. Returned by
/// [`crate::ExecutorBuilder::admission_check`]'s closure parameter.
///
/// # Example
///
/// ```no_run
/// use taktora_executor::{Executor, IntegrityLevel};
///
/// let mut exec = Executor::builder()
///     .integrity_level(IntegrityLevel::SafetyCritical)
///     .admission_check(|ctx| {
///         // Integrator-provided spatial-isolation check (example).
///         // Real implementations would verify allocator lock,
///         // channel topology, etc.
///         ctx.verify_isolation()
///     })
///     .build()
///     .unwrap();
/// ```
pub struct AdmissionContext {
    integrity_level: Option<IntegrityLevel>,
    task_count: usize,
}

impl AdmissionContext {
    /// Build an admission context from executor state.
    ///
    /// # Internal Use
    ///
    /// This constructor is invoked by the executor at cold-start time; user
    /// code receives an already-built `&AdmissionContext` in the admission
    /// check closure.
    #[must_use]
    pub(crate) const fn new(integrity_level: Option<IntegrityLevel>, task_count: usize) -> Self {
        Self {
            integrity_level,
            task_count,
        }
    }

    /// The executor's pinned integrity level, if configured via
    /// [`crate::ExecutorBuilder::integrity_level`].
    ///
    /// Returns `None` if the executor is unpinned (mixed integrity allowed).
    #[must_use]
    pub const fn integrity_level(&self) -> Option<IntegrityLevel> {
        self.integrity_level
    }

    /// Number of registered tasks at the time of this admission check.
    #[must_use]
    pub const fn task_count(&self) -> usize {
        self.task_count
    }

    /// Default spatial-isolation verification.
    ///
    /// Returns [`AdmissionOutcome::Admitted`] unconditionally. Integrators
    /// may call this as a base implementation and extend it with their own
    /// checks, or replace it entirely with a custom verification procedure.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use taktora_executor::Executor;
    ///
    /// let mut exec = Executor::builder()
    ///     .admission_check(|ctx| {
    ///         // Use the default check (always admits).
    ///         ctx.verify_isolation()
    ///     })
    ///     .build()
    ///     .unwrap();
    /// ```
    #[must_use]
    pub const fn verify_isolation(&self) -> AdmissionOutcome {
        AdmissionOutcome::Admitted
    }
}
