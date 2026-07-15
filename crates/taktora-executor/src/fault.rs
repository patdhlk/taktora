//! Cycle-overrun fault primitive — implements `FEAT_0018` (`REQ_0070`,
//! `REQ_0071`, `REQ_0073`) and feeds `REQ_0102` (overrun counter).
//!
//! State machines are `AtomicU64`-packed so the dispatch hot path
//! (`REQ_0060`, `REQ_0104`) can read/write them wait-free without
//! `Mutex` or allocation.
//!
//! `BB_0093`.

// The `FaultAtomic` / `ExecutorFaultAtomic` packed-atomic storage is
// `pub(crate)` for use by `executor.rs` / `TaskEntry` in later tasks
// (the cycle-overrun fault primitive lands in stages — BB_0093 ships
// the state-machine module first; integration follows in Task 6+).
// Until then, the storage types are unused — silence the dead-code
// and redundant-pub-crate lints uniformly.
#![allow(dead_code)]
#![allow(clippy::redundant_pub_crate)]

use core::sync::atomic::{AtomicU64, Ordering};
use core::time::Duration;
use std::time::Instant;

/// Per-task fault state. Stored as packed `AtomicU64` in `TaskEntry`;
/// the public API hands callers this snapshot view.
///
/// Not to be confused with [`ExecutorFaultState`], which is the
/// executor-wide counterpart; this one is scoped to a single task.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultState {
    /// Task is healthy and dispatches normally.
    Running,
    /// Task is faulted; main item is not dispatched until cleared.
    Faulted {
        /// Why the task transitioned to `Faulted`.
        reason: FaultReason,
        /// Approximate transition time, executor-relative milliseconds.
        /// Resolve to `Instant` via the executor's `start_time`.
        since_ms: u32,
    },
}

/// Why a task transitioned to `Faulted`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultReason {
    /// The task's `execute()` ran longer than its declared budget.
    /// `took_ms` is saturated to `u32::MAX` ms (~49.7 days).
    BudgetExceeded {
        /// Observed execution time in ms (saturated).
        took_ms: u32,
        /// Declared budget in ms (saturated).
        budget_ms: u32,
    },
    /// The executor entered `Faulted` while this task was `Running`.
    /// The cascade transition is automatic; per-task observers do
    /// not fire (only `Observer::on_executor_fault` does).
    ExecutorFaulted,
}

/// Executor-wide fault state.
///
/// Not to be confused with [`FaultState`], which is the per-task
/// counterpart; this one spans the whole executor and halts all tasks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutorFaultState {
    /// Executor is healthy; all tasks dispatch normally.
    Running,
    /// Executor-wide budget breached. All tasks halt (or route to
    /// their handlers) until cleared.
    Faulted {
        /// Why the executor transitioned to `Faulted`.
        reason: ExecutorFaultReason,
        /// Approximate transition time, executor-relative milliseconds.
        since_ms: u32,
    },
}

/// Why the executor transitioned to `Faulted`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutorFaultReason {
    /// A task's `execute()` exceeded the executor-wide iteration budget.
    /// `task_idx` is the internal task-table index; resolve to `TaskId`
    /// via the executor's task table.
    IterationBudgetExceeded {
        /// Internal index of the offending task in the executor's task vec.
        task_idx: u32,
        /// Observed execution time in ms (saturated).
        took_ms: u32,
        /// Declared executor-wide budget in ms (saturated).
        budget_ms: u32,
    },
}

/// Wait-free packed storage for `FaultState`.
///
/// Layout (bit positions, MSB to LSB):
///   63..62 (2 bits)  — discriminant:
///       0 = Running
///       1 = Faulted{BudgetExceeded}
///       2 = Faulted{ExecutorFaulted}
///       3 = reserved
///   61..32 (30 bits) — `took_ms` (saturated; 0 for `ExecutorFaulted`)
///   31..0  (32 bits) — `since_ms`
///
/// `budget_ms` is stored separately (it's a static property of the task,
/// not a runtime measurement), so it's reconstructed by the `unpack`
/// caller from the task's stored `budget`.
#[derive(Debug, Default)]
pub(crate) struct FaultAtomic(AtomicU64);

impl FaultAtomic {
    /// Construct a fresh `FaultAtomic` in the `Running` state.
    pub(crate) const fn new() -> Self {
        Self(AtomicU64::new(0))
    }

    /// Pack `state` into a u64.
    #[allow(clippy::cast_possible_truncation)]
    pub(crate) fn pack(state: FaultState) -> u64 {
        match state {
            FaultState::Running => 0,
            FaultState::Faulted { reason, since_ms } => {
                let (disc, took_ms) = match reason {
                    FaultReason::BudgetExceeded { took_ms, .. } => (1_u64, took_ms),
                    FaultReason::ExecutorFaulted => (2_u64, 0_u32),
                };
                (disc << 62) | ((u64::from(took_ms) & 0x3FFF_FFFF) << 32) | u64::from(since_ms)
            }
        }
    }

    /// Recover `FaultState` from a packed u64 + the task's stored budget.
    #[allow(clippy::cast_possible_truncation)]
    pub(crate) const fn unpack(packed: u64, budget_ms: u32) -> FaultState {
        let disc = (packed >> 62) & 0x3;
        let took_ms = ((packed >> 32) & 0x3FFF_FFFF) as u32;
        let since_ms = (packed & 0xFFFF_FFFF) as u32;
        match disc {
            1 => FaultState::Faulted {
                reason: FaultReason::BudgetExceeded { took_ms, budget_ms },
                since_ms,
            },
            2 => FaultState::Faulted {
                reason: FaultReason::ExecutorFaulted,
                since_ms,
            },
            // 0 = Running; 3 = reserved, treat as Running
            _ => FaultState::Running,
        }
    }

    /// Load the current state.
    pub(crate) fn load(&self, budget_ms: u32) -> FaultState {
        Self::unpack(self.0.load(Ordering::Acquire), budget_ms)
    }

    /// Store `state` and return the previous state. Callers use the
    /// returned value to detect "first transition" (Observer callback
    /// fires) vs "redundant store" (no callback).
    pub(crate) fn swap(&self, state: FaultState, budget_ms: u32) -> FaultState {
        Self::unpack(self.0.swap(Self::pack(state), Ordering::AcqRel), budget_ms)
    }
}

/// Wait-free packed storage for `ExecutorFaultState`. Same shape as
/// `FaultAtomic`; the offending task is stored as a u32 index in
/// adjacent atomics (see `Executor` storage in Task 6).
///
/// Layout:
///   63..62 (2 bits)  — discriminant:
///       0 = Running
///       1 = Faulted{IterationBudgetExceeded}
///       2..3 = reserved
///   61..32 (30 bits) — `took_ms` (saturated)
///   31..0  (32 bits) — `since_ms`
#[derive(Debug, Default)]
pub(crate) struct ExecutorFaultAtomic(AtomicU64);

impl ExecutorFaultAtomic {
    /// Construct a fresh `ExecutorFaultAtomic` in the `Running` state.
    pub(crate) const fn new() -> Self {
        Self(AtomicU64::new(0))
    }

    /// Pack `state` into a u64.
    #[allow(clippy::cast_possible_truncation)]
    pub(crate) fn pack(state: ExecutorFaultState) -> u64 {
        match state {
            ExecutorFaultState::Running => 0,
            ExecutorFaultState::Faulted { reason, since_ms } => {
                let took_ms = match reason {
                    ExecutorFaultReason::IterationBudgetExceeded { took_ms, .. } => took_ms,
                };
                (1_u64 << 62) | ((u64::from(took_ms) & 0x3FFF_FFFF) << 32) | u64::from(since_ms)
            }
        }
    }

    /// Recover `ExecutorFaultState`. `task_idx` and `budget_ms` are
    /// supplied externally — they live in adjacent atomics next to
    /// this one on the `Executor` (see Task 6).
    #[allow(clippy::cast_possible_truncation)]
    pub(crate) const fn unpack(packed: u64, task_idx: u32, budget_ms: u32) -> ExecutorFaultState {
        let disc = (packed >> 62) & 0x3;
        let took_ms = ((packed >> 32) & 0x3FFF_FFFF) as u32;
        let since_ms = (packed & 0xFFFF_FFFF) as u32;
        match disc {
            1 => ExecutorFaultState::Faulted {
                reason: ExecutorFaultReason::IterationBudgetExceeded {
                    task_idx,
                    took_ms,
                    budget_ms,
                },
                since_ms,
            },
            // 0 = Running; 2..=3 = reserved, treat as Running
            _ => ExecutorFaultState::Running,
        }
    }

    /// Load the current state.
    pub(crate) fn load(&self, task_idx: u32, budget_ms: u32) -> ExecutorFaultState {
        Self::unpack(self.0.load(Ordering::Acquire), task_idx, budget_ms)
    }

    /// Store `state` and return the previous state.
    pub(crate) fn swap(
        &self,
        state: ExecutorFaultState,
        task_idx: u32,
        budget_ms: u32,
    ) -> ExecutorFaultState {
        Self::unpack(
            self.0.swap(Self::pack(state), Ordering::AcqRel),
            task_idx,
            budget_ms,
        )
    }
}

/// Helper: convert a `Duration` to ms, saturated to `u32::MAX`.
#[allow(clippy::cast_possible_truncation)]
pub(crate) fn duration_to_ms_sat(d: Duration) -> u32 {
    let ms = d.as_millis();
    if ms > u128::from(u32::MAX) {
        u32::MAX
    } else {
        ms as u32
    }
}

/// Helper: convert an executor-relative `Instant` to ms since start,
/// saturated to `u32::MAX`.
pub(crate) fn instant_to_since_ms(at: Instant, start: Instant) -> u32 {
    duration_to_ms_sat(at.saturating_duration_since(start))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_unpack_running() {
        let s = FaultState::Running;
        let p = FaultAtomic::pack(s);
        assert_eq!(p, 0);
        assert_eq!(FaultAtomic::unpack(p, 100), FaultState::Running);
    }

    #[test]
    fn pack_unpack_budget_exceeded_round_trip() {
        let s = FaultState::Faulted {
            reason: FaultReason::BudgetExceeded {
                took_ms: 17,
                budget_ms: 10,
            },
            since_ms: 1_234,
        };
        let p = FaultAtomic::pack(s);
        // budget_ms is provided externally on unpack; pack stored took_ms only
        let restored = FaultAtomic::unpack(p, 10);
        assert_eq!(restored, s);
    }

    #[test]
    fn pack_unpack_executor_faulted_round_trip() {
        let s = FaultState::Faulted {
            reason: FaultReason::ExecutorFaulted,
            since_ms: u32::MAX,
        };
        let p = FaultAtomic::pack(s);
        // budget_ms is unused for this discriminant
        let restored = FaultAtomic::unpack(p, 999);
        assert_eq!(restored, s);
    }

    #[test]
    fn pack_unpack_saturates_took_ms() {
        let s = FaultState::Faulted {
            reason: FaultReason::BudgetExceeded {
                took_ms: 0x3FFF_FFFF,
                budget_ms: 5,
            },
            since_ms: 42,
        };
        let p = FaultAtomic::pack(s);
        let restored = FaultAtomic::unpack(p, 5);
        assert_eq!(restored, s);
    }

    #[test]
    fn fault_atomic_swap_returns_previous() {
        let fa = FaultAtomic::new();
        assert_eq!(fa.load(0), FaultState::Running);
        let prev = fa.swap(
            FaultState::Faulted {
                reason: FaultReason::BudgetExceeded {
                    took_ms: 5,
                    budget_ms: 3,
                },
                since_ms: 100,
            },
            3,
        );
        assert_eq!(prev, FaultState::Running);
        let prev = fa.swap(FaultState::Running, 3);
        assert!(matches!(
            prev,
            FaultState::Faulted {
                reason: FaultReason::BudgetExceeded { .. },
                ..
            }
        ));
    }

    #[test]
    fn executor_fault_atomic_swap_returns_previous() {
        let efa = ExecutorFaultAtomic::new();
        assert_eq!(efa.load(0, 0), ExecutorFaultState::Running);
        let prev = efa.swap(
            ExecutorFaultState::Faulted {
                reason: ExecutorFaultReason::IterationBudgetExceeded {
                    task_idx: 3,
                    took_ms: 20,
                    budget_ms: 10,
                },
                since_ms: 50,
            },
            3,
            10,
        );
        assert_eq!(prev, ExecutorFaultState::Running);
    }

    #[test]
    fn duration_helpers_saturate() {
        assert_eq!(duration_to_ms_sat(Duration::from_millis(5)), 5);
        let huge = Duration::from_secs(60 * 60 * 24 * 365 * 100); // 100 years
        assert_eq!(duration_to_ms_sat(huge), u32::MAX);
    }
}
