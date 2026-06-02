#![no_std]
#![warn(missing_docs)]
//! POD NC<->commander ABI (`REQ_0855`). The `AXIS_REF` analogue: the
//! commander runs `PLCopen` function blocks and correlates by `token`; the
//! NC publishes `AxisStatus` every cycle. `#[repr(C)]`, `Copy`, no heap.
//!
//! # Caveat for cross-process readers
//!
//! These types cross a shared-memory boundary (iceoryx2) as raw bytes. A
//! reader must validate every `#[repr(u8)]` discriminant (`CommandKind`,
//! `AxisState`, `TokenState`) before matching on it: the transport performs
//! no bounds check, and matching a transmuted out-of-range discriminant is
//! undefined behaviour. This crate is type definitions only — the
//! validation is the consumer's responsibility (the NC command reader).

/// Axis identifier within one NC process.
pub type AxisId = u16;
/// Monotonic command token; a new token is the rising edge of execution.
pub type Token = u32;

/// `PLCopen`-verb command kind (`BufferMode` is Aborting-only in v1).
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandKind {
    /// `MC_Power` — walk to `OperationEnabled`.
    Power,
    /// `MC_Reset` — clear fault, return to `Standstill`.
    Reset,
    /// `MC_Stop` — controlled stop, axis stays busy until new token.
    Stop,
    /// `MC_Halt` — controlled stop, re-commandable.
    Halt,
    /// `MC_MoveVelocity`.
    MoveVelocity,
    /// `MC_MoveAbsolute`.
    MoveAbsolute,
    /// `MC_MoveRelative`.
    MoveRelative,
    /// `MC_GearIn`.
    GearIn,
    /// `MC_CamIn`.
    CamIn,
    /// Flying-saw engage.
    FlyingSaw,
    /// `MC_MoveSuperimposed`.
    Superimpose,
}

/// Fixed POD parameter block (unused fields are ignored per `kind`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct CommandParams {
    /// Target position (absolute/relative moves, cam/gear master ref).
    pub target_pos: f64,
    /// Target / limit velocity.
    pub velocity: f64,
    /// Acceleration limit.
    pub accel: f64,
    /// Jerk limit (S-curve / quintic).
    pub jerk: f64,
}

/// One command into the NC process.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct AxisCommand {
    /// Target axis.
    pub axis_id: AxisId,
    /// Correlation token (rising edge of execution).
    pub token: Token,
    /// What to do.
    pub kind: CommandKind,
    /// Parameters.
    pub params: CommandParams,
}

/// `PLCopen`-ish axis state, published each cycle.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AxisState {
    /// Power off.
    Disabled,
    /// Enabled, at rest, no active motion.
    Standstill,
    /// Executing a discrete (point-to-point) move.
    DiscreteMotion,
    /// Executing a continuous (velocity) move.
    ContinuousMotion,
    /// Following a master (gear/cam/flying-saw).
    SynchronizedMotion,
    /// Performing a commanded stop.
    ///
    /// Reserved: not yet produced by the NC (a `Stop`/`Halt` ramp currently
    /// publishes `ContinuousMotion`). Its discriminant position is
    /// ABI-load-bearing — do not reorder or remove.
    Stopping,
    /// Faulted; quickstopping or stopped.
    ErrorStop,
}

/// Lifecycle of the last-seen `token` (commander reconstructs `PLCopen`
/// `Done`/`Busy`/`CommandAborted` from this).
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenState {
    /// No command in flight.
    Idle,
    /// Command accepted and executing.
    Active,
    /// Command completed.
    Done,
    /// Superseded by a newer token or a fault.
    Aborted,
    /// Rejected (e.g. infeasible) — see `error_code`.
    Error,
}

/// Per-axis status, published every cycle (`REQ_0855`).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct AxisStatus {
    /// Axis this status is for.
    pub axis_id: AxisId,
    /// `PLCopen`-ish state.
    pub state: AxisState,
    /// Token-lifecycle of `last_token`.
    pub token_state: TokenState,
    /// Most recent token the NC has seen for this axis.
    pub last_token: Token,
    /// Unwrapped actual position (engineering units).
    pub actual_pos: f64,
    /// Actual velocity (engineering units/s).
    pub actual_vel: f64,
    /// Commanded position this cycle (engineering units).
    pub cmd_pos: f64,
    /// Drive/axis error code, 0 = none.
    pub error_code: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::size_of;

    #[test]
    fn types_are_pod_copy() {
        fn assert_copy<T: Copy>() {}
        assert_copy::<AxisCommand>();
        assert_copy::<AxisStatus>();
    }

    #[test]
    fn command_round_trips_a_velocity_move() {
        let c = AxisCommand {
            axis_id: 3,
            token: 99,
            kind: CommandKind::MoveVelocity,
            params: CommandParams {
                target_pos: 0.0,
                velocity: 25.0,
                accel: 100.0,
                jerk: 0.0,
            },
        };
        assert_eq!(c.axis_id, 3);
        assert_eq!(c.kind, CommandKind::MoveVelocity);
        assert!((c.params.velocity - 25.0).abs() < f64::EPSILON);
    }

    #[test]
    fn abi_sizes_are_stable() {
        // Guards accidental ABI drift; update deliberately if layout changes.
        assert_eq!(size_of::<AxisStatus>(), 40);
        // `AxisCommand` carries `repr(C)` tail/interior padding (uninitialised
        // bytes the transport copies verbatim); the guard makes that visible
        // and catches layout drift.
        assert_eq!(size_of::<AxisCommand>(), 48);
    }
}
