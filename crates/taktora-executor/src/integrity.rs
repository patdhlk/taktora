//! Context-based integrity isolation primitives (`TSR_0003`).

/// Integrity level governing task isolation boundaries.
///
/// When an executor is pinned to a specific level via
/// [`crate::ExecutorBuilder::integrity_level`], the executor rejects any task
/// whose declared level differs from the pin, enforcing a single-level
/// execution context (`TSR_0003`).
///
/// **Default**: [`IntegrityLevel::QualityManaged`] (both for the item trait
/// method and for unpinned executors).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IntegrityLevel {
    /// Safety-critical context — the highest integrity partition.
    SafetyCritical,
    /// Quality-managed context — the standard operating partition.
    #[default]
    QualityManaged,
}
