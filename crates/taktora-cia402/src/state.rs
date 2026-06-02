//! `CiA` 402 controlword/statusword bit semantics (`REQ_0857`).

/// Controlword command values (`CiA` 402 §10.3.1).
pub mod controlword {
    /// Shutdown: -> `ReadyToSwitchOn` (valid from `SwitchOnDisabled`,
    /// `SwitchedOn`, and `OperationEnabled`).
    pub const SHUTDOWN: u16 = 0x0006;
    /// Switch on: `ReadyToSwitchOn` -> `SwitchedOn`.
    pub const SWITCH_ON: u16 = 0x0007;
    /// Enable operation: `SwitchedOn` -> `OperationEnabled` (and hold).
    pub const ENABLE_OPERATION: u16 = 0x000F;
    /// Quick stop: -> `QuickStopActive` (drive runs `0x6085` decel).
    pub const QUICK_STOP: u16 = 0x0002;
    /// Disable voltage: -> `SwitchOnDisabled`.
    pub const DISABLE_VOLTAGE: u16 = 0x0000;
    /// Fault reset (rising edge of bit 7).
    pub const FAULT_RESET: u16 = 0x0080;
}

/// `CiA` 402 drive state, decoded from the statusword (`CiA` 402 §10.1.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cia402State {
    /// Not ready to switch on.
    NotReadyToSwitchOn,
    /// Switch on disabled.
    SwitchOnDisabled,
    /// Ready to switch on.
    ReadyToSwitchOn,
    /// Switched on.
    SwitchedOn,
    /// Operation enabled (setpoints accepted).
    OperationEnabled,
    /// Quick stop active.
    QuickStopActive,
    /// Fault reaction active.
    FaultReactionActive,
    /// Fault.
    Fault,
}

/// Decode the standard `CiA` 402 statusword bit pattern.
#[must_use]
pub const fn decode_state(sw: u16) -> Cia402State {
    // Masks per `CiA` 402: some states use 0x6F, others 0x4F.
    if sw & 0x4F == 0x40 {
        Cia402State::SwitchOnDisabled
    } else if sw & 0x6F == 0x21 {
        Cia402State::ReadyToSwitchOn
    } else if sw & 0x6F == 0x23 {
        Cia402State::SwitchedOn
    } else if sw & 0x6F == 0x27 {
        Cia402State::OperationEnabled
    } else if sw & 0x6F == 0x07 {
        Cia402State::QuickStopActive
    } else if sw & 0x4F == 0x0F {
        Cia402State::FaultReactionActive
    } else if sw & 0x4F == 0x08 {
        Cia402State::Fault
    } else {
        Cia402State::NotReadyToSwitchOn
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_canonical_statuswords() {
        assert_eq!(decode_state(0x0250), Cia402State::SwitchOnDisabled); // x1xx0000
        assert_eq!(decode_state(0x0231), Cia402State::ReadyToSwitchOn); // x01x0001
        assert_eq!(decode_state(0x0233), Cia402State::SwitchedOn); // x01x0011
        assert_eq!(decode_state(0x0237), Cia402State::OperationEnabled); // x01x0111
        assert_eq!(decode_state(0x0207), Cia402State::QuickStopActive); // x00x0111
        assert_eq!(decode_state(0x020F), Cia402State::FaultReactionActive); // x0xx1111
        assert_eq!(decode_state(0x0208), Cia402State::Fault); // x0xx1000
        assert_eq!(decode_state(0x0000), Cia402State::NotReadyToSwitchOn); // x0xx0000
    }
}
