//! Hand-written EL7047 "Positioning interface" PDO codec. The ESI codegen
//! cannot model this device's selectable PDO assignments (it emits the union
//! of all Fixed PDOs), so we encode/decode the chosen assignment's image by
//! hand. Layout verified against esi/beckhoff_el7047.xml (rev 0x00170000):
//! Rx 0x1601+0x1602+0x1606 = 22 bytes; Tx 0x1a01+0x1a03+0x1a07 = 24 bytes.

/// Output (RxPDO) process image for the positioning-interface assignment.
pub const OUTPUT_LEN: usize = 22;
/// Input (TxPDO) process image for the positioning-interface assignment.
pub const INPUT_LEN: usize = 24;

/// Beckhoff POS "Start type" values (from the ESI DT0808EN16 enum).
pub mod start_type {
    /// Relative move: the target position is a delta from the current one.
    pub const RELATIVE: u16 = 2;
}

/// Decoded EL7047 status (subset we care about).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct El7047Status {
    /// STM Status bit1: drive is enabled and ready to move.
    pub ready: bool,
    /// STM Status bit3: drive fault latched.
    pub error: bool,
    /// STM Status bit2: non-fatal warning active.
    pub warning: bool,
    /// STM Status bit7: motor stall detected.
    pub motor_stall: bool,
    /// POS Status bit0: a positioning move is in progress.
    pub busy: bool,
    /// POS Status bit1: the commanded position has been reached.
    pub in_target: bool,
    /// POS Status bit7: ready to accept a new Execute pulse.
    pub ready_to_execute: bool,
    /// Actual position counter (increments).
    pub actual_position: i32,
}

/// Control surface we write each cycle.
#[derive(Clone, Copy, Debug, Default)]
pub struct El7047Control {
    /// STM Control bit0: enable the drive.
    pub enable: bool,
    /// STM Control bit1: reset a latched fault.
    pub reset: bool,
    /// POS Control bit0: pulse to start the configured move.
    pub execute: bool,
    /// POS Control bit1: emergency-stop the drive.
    pub emergency_stop: bool,
    /// Target position (delta when `start_type` is Relative).
    pub target_position: i32,
    /// Move velocity (POS-interface raw units).
    pub velocity: i16,
    /// Start-type selector (see [`start_type`]).
    pub start_type: u16,
    /// Acceleration ramp (POS-interface raw units).
    pub acceleration: u16,
    /// Deceleration ramp (POS-interface raw units).
    pub deceleration: u16,
}

/// Encode the control surface into the 22-byte positioning-interface
/// output image. Bytes 0-5 (ENC Control) stay zero.
#[must_use]
pub fn encode_control(c: &El7047Control) -> [u8; OUTPUT_LEN] {
    let mut img = [0u8; OUTPUT_LEN];
    // STM Control word (bytes 6-7): bit0 Enable, bit1 Reset, bit2 Reduce-torque.
    let mut stm = 0u16;
    if c.enable {
        stm |= 1 << 0;
    }
    if c.reset {
        stm |= 1 << 1;
    }
    img[6..8].copy_from_slice(&stm.to_le_bytes());
    // POS Control word (bytes 8-9): bit0 Execute, bit1 Emergency-stop.
    let mut pos = 0u16;
    if c.execute {
        pos |= 1 << 0;
    }
    if c.emergency_stop {
        pos |= 1 << 1;
    }
    img[8..10].copy_from_slice(&pos.to_le_bytes());
    img[10..14].copy_from_slice(&c.target_position.to_le_bytes());
    img[14..16].copy_from_slice(&c.velocity.to_le_bytes());
    img[16..18].copy_from_slice(&c.start_type.to_le_bytes());
    img[18..20].copy_from_slice(&c.acceleration.to_le_bytes());
    img[20..22].copy_from_slice(&c.deceleration.to_le_bytes());
    img
}

/// Decode the 24-byte positioning-interface input image. Returns `None`
/// if the buffer is too short.
#[must_use]
pub fn decode_status(img: &[u8]) -> Option<El7047Status> {
    if img.len() < INPUT_LEN {
        return None;
    }
    let stm = u16::from_le_bytes([img[10], img[11]]);
    let pos = u16::from_le_bytes([img[12], img[13]]);
    Some(El7047Status {
        ready: stm & (1 << 1) != 0,
        warning: stm & (1 << 2) != 0,
        error: stm & (1 << 3) != 0,
        motor_stall: stm & (1 << 7) != 0,
        busy: pos & (1 << 0) != 0,
        in_target: pos & (1 << 1) != 0,
        ready_to_execute: pos & (1 << 7) != 0,
        actual_position: i32::from_le_bytes([img[14], img[15], img[16], img[17]]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_sets_enable_and_relative_move() {
        let ctrl = El7047Control {
            enable: true,
            execute: true,
            target_position: 3200,
            velocity: 1000,
            start_type: start_type::RELATIVE,
            acceleration: 1000,
            deceleration: 1000,
            ..Default::default()
        };
        let img = encode_control(&ctrl);
        assert_eq!(img.len(), OUTPUT_LEN);
        // STM Control word bytes 6-7, bit0 = Enable.
        assert_eq!(u16::from_le_bytes([img[6], img[7]]) & 0x0001, 0x0001);
        // POS Control word bytes 8-9, bit0 = Execute.
        assert_eq!(u16::from_le_bytes([img[8], img[9]]) & 0x0001, 0x0001);
        // Target position bytes 10-13.
        assert_eq!(
            i32::from_le_bytes([img[10], img[11], img[12], img[13]]),
            3200
        );
        // Start type bytes 16-17 = Relative.
        assert_eq!(u16::from_le_bytes([img[16], img[17]]), 2);
        // Velocity bytes 14-15.
        assert_eq!(i16::from_le_bytes([img[14], img[15]]), 1000);
    }

    #[test]
    fn encode_emergency_stop_sets_pos_control_bit1() {
        let ctrl = El7047Control {
            emergency_stop: true,
            ..Default::default()
        };
        let img = encode_control(&ctrl);
        assert_eq!(u16::from_le_bytes([img[8], img[9]]) & 0x0002, 0x0002);
    }

    #[test]
    fn decode_reads_status_and_position() {
        let mut img = [0u8; INPUT_LEN];
        // STM Status bytes 10-11: Ready (bit1).
        img[10] = 0b0000_0010;
        // POS Status bytes 12-13: In-Target (bit1).
        img[12] = 0b0000_0010;
        // Actual position bytes 14-17 = -1500.
        img[14..18].copy_from_slice(&(-1500i32).to_le_bytes());
        let st = decode_status(&img).expect("len ok");
        assert!(st.ready);
        assert!(st.in_target);
        assert!(!st.error);
        assert_eq!(st.actual_position, -1500);
    }

    #[test]
    fn decode_rejects_short_buffer() {
        assert!(decode_status(&[0u8; 4]).is_none());
    }
}
