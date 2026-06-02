//! Per-axis `CiA` 402 power state machine (`REQ_0857`).
//!
//! Stateless w.r.t. the drive — it reads the live statusword each cycle and
//! produces the controlword that advances toward (or holds) the target. The
//! glue ticks it every cycle.

use crate::state::{Cia402State, controlword as cw, decode_state};

/// Where the operator wants this axis to be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerTarget {
    /// Walk to and hold `OperationEnabled`.
    Enabled,
    /// Command Quick Stop (the safe-state reaction, `REQ_0861`).
    QuickStop,
    /// Disable the drive.
    Disabled,
}

/// Drives the controlword toward [`PowerTarget`] from the live statusword.
#[derive(Clone, Copy, Debug)]
pub struct PowerStateMachine {
    target: PowerTarget,
}

impl PowerStateMachine {
    /// New machine aiming at `target`.
    #[must_use]
    pub const fn new(target: PowerTarget) -> Self {
        Self { target }
    }

    /// Retarget (e.g. operator `Power`/`Reset`, or a fault -> `QuickStop`).
    pub const fn set_target(&mut self, target: PowerTarget) {
        self.target = target;
    }

    /// Current target.
    #[must_use]
    pub const fn target(&self) -> PowerTarget {
        self.target
    }

    /// Controlword to write this cycle given `statusword`.
    #[must_use]
    pub const fn next_controlword(&self, statusword: u16) -> u16 {
        let state = decode_state(statusword);
        // A fault always clears first, regardless of target.
        if matches!(state, Cia402State::Fault | Cia402State::FaultReactionActive) {
            return cw::FAULT_RESET;
        }
        match self.target {
            PowerTarget::Disabled => cw::DISABLE_VOLTAGE,
            PowerTarget::QuickStop => match state {
                Cia402State::OperationEnabled | Cia402State::QuickStopActive => cw::QUICK_STOP,
                _ => cw::DISABLE_VOLTAGE,
            },
            PowerTarget::Enabled => match state {
                Cia402State::ReadyToSwitchOn => cw::SWITCH_ON,
                Cia402State::SwitchedOn | Cia402State::OperationEnabled => cw::ENABLE_OPERATION,
                Cia402State::QuickStopActive => cw::DISABLE_VOLTAGE, // re-arm via SwitchOnDisabled
                // SwitchOnDisabled, NotReadyToSwitchOn, and any unknown state -> SHUTDOWN
                _ => cw::SHUTDOWN,
            },
        }
    }

    /// `true` once the live statusword reports `OperationEnabled`.
    ///
    /// A pure statusword query: it needs no `self` (the target is
    /// irrelevant), so call it as `PowerStateMachine::is_enabled(sw)`.
    #[must_use]
    pub const fn is_enabled(statusword: u16) -> bool {
        matches!(decode_state(statusword), Cia402State::OperationEnabled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Cia402State, controlword as cw};

    fn sw_for(s: Cia402State) -> u16 {
        match s {
            Cia402State::SwitchOnDisabled => 0x0250,
            Cia402State::ReadyToSwitchOn => 0x0231,
            Cia402State::SwitchedOn => 0x0233,
            Cia402State::OperationEnabled => 0x0237,
            Cia402State::QuickStopActive => 0x0207,
            Cia402State::Fault => 0x0208,
            _ => 0x0000,
        }
    }

    #[test]
    fn walks_disabled_to_enabled() {
        let m = PowerStateMachine::new(PowerTarget::Enabled);
        assert_eq!(
            m.next_controlword(sw_for(Cia402State::SwitchOnDisabled)),
            cw::SHUTDOWN
        );
        assert_eq!(
            m.next_controlword(sw_for(Cia402State::ReadyToSwitchOn)),
            cw::SWITCH_ON
        );
        assert_eq!(
            m.next_controlword(sw_for(Cia402State::SwitchedOn)),
            cw::ENABLE_OPERATION
        );
        assert_eq!(
            m.next_controlword(sw_for(Cia402State::OperationEnabled)),
            cw::ENABLE_OPERATION
        );
    }

    #[test]
    fn fault_is_reset_first() {
        let m = PowerStateMachine::new(PowerTarget::Enabled);
        assert_eq!(
            m.next_controlword(sw_for(Cia402State::Fault)),
            cw::FAULT_RESET
        );
    }

    #[test]
    fn quickstop_target_commands_quick_stop_when_enabled() {
        let m = PowerStateMachine::new(PowerTarget::QuickStop);
        assert_eq!(
            m.next_controlword(sw_for(Cia402State::OperationEnabled)),
            cw::QUICK_STOP
        );
    }

    #[test]
    fn quickstop_holds_when_already_in_quickstop_active() {
        let m = PowerStateMachine::new(PowerTarget::QuickStop);
        assert_eq!(
            m.next_controlword(sw_for(Cia402State::QuickStopActive)),
            cw::QUICK_STOP
        );
    }

    #[test]
    fn disabled_target_always_disables_voltage() {
        let m = PowerStateMachine::new(PowerTarget::Disabled);
        assert_eq!(
            m.next_controlword(sw_for(Cia402State::OperationEnabled)),
            cw::DISABLE_VOLTAGE
        );
    }

    #[test]
    fn fault_reset_then_rewalk() {
        // A fault clears first; once cleared, the machine resumes walking up.
        let m = PowerStateMachine::new(PowerTarget::Enabled);
        assert_eq!(
            m.next_controlword(sw_for(Cia402State::Fault)),
            cw::FAULT_RESET
        );
        assert_eq!(
            m.next_controlword(sw_for(Cia402State::SwitchOnDisabled)),
            cw::SHUTDOWN
        );
    }
}
