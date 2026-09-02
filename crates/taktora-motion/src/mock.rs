//! Host-side virtual `CSP` drive behind [`CyclicFieldbus`] (`REQ_0865`).
//!
//! Primary test vehicle and simulation backend v0 — structured to grow
//! (per-axis dynamics, load models) without changing the seam.

use taktora_cia402::Cia402Drive;
use taktora_cia402::state::{Cia402State, controlword as cw, decode_state};
use taktora_cyclic_fieldbus::{CycleQuality, CyclicFieldbus, Validity};

/// Bytes per axis in the flat mock image:
/// outputs `[cw:2][mode:1][target:4]` (bytes 0..7),
/// inputs `[sw:2][actual:4]` (bytes 7..13).
const BYTES_PER_AXIS: usize = 13;

/// Routing into the mock image: an axis index.
#[derive(Clone, Copy, Debug)]
pub struct MockRouting {
    /// Axis index.
    pub axis: u16,
}

/// [`Cia402Drive`] over one axis's 13-byte slice of the mock image.
#[derive(Clone, Copy, Debug)]
pub struct MockDrive {
    base: usize,
}

impl MockDrive {
    /// Accessor for `axis`.
    #[must_use]
    pub const fn for_axis(axis: u16) -> Self {
        // Index bounded by the caller's axis count; no overflow possible
        // within a realistic number of axes.
        #[allow(clippy::cast_possible_truncation)]
        Self {
            base: axis as usize * BYTES_PER_AXIS,
        }
    }
}

impl Cia402Drive for MockDrive {
    type Image = [u8];

    fn statusword(&self, img: &[u8]) -> u16 {
        u16::from_le_bytes([img[self.base + 7], img[self.base + 8]])
    }

    fn set_controlword(&self, img: &mut [u8], v: u16) {
        img[self.base..self.base + 2].copy_from_slice(&v.to_le_bytes());
    }

    fn actual_position(&self, img: &[u8]) -> i32 {
        let b = self.base + 9;
        i32::from_le_bytes([img[b], img[b + 1], img[b + 2], img[b + 3]])
    }

    fn set_target_position(&self, img: &mut [u8], p: i32) {
        img[self.base + 3..self.base + 7].copy_from_slice(&p.to_le_bytes());
    }

    fn set_mode(&self, img: &mut [u8], m: u8) {
        img[self.base + 2] = m;
    }
}

/// Virtual multi-axis `CSP` bus implementing [`CyclicFieldbus`].
pub struct MockCyclicFieldbus {
    n: usize,
    img: Vec<u8>,
    cycle: u64,
    faulted: Vec<bool>,
    stale: Vec<bool>,
}

impl MockCyclicFieldbus {
    /// New bus with `n` perfect `CSP` drives, each in `SwitchOnDisabled`.
    #[must_use]
    pub fn new(n: usize) -> Self {
        let mut img = vec![0u8; n * BYTES_PER_AXIS];
        for a in 0..n {
            // Index bounded by n; no overflow within realistic axis counts.
            #[allow(clippy::cast_possible_truncation)]
            let d = MockDrive::for_axis(a as u16);
            d.set_controlword(&mut img, 0);
            // Initialise statusword to SwitchOnDisabled (0x0250).
            img[a * BYTES_PER_AXIS + 7..a * BYTES_PER_AXIS + 9]
                .copy_from_slice(&0x0250u16.to_le_bytes());
        }
        Self {
            n,
            img,
            cycle: 0,
            faulted: vec![false; n],
            stale: vec![false; n],
        }
    }

    /// Read-only image access (test helper).
    pub fn with_image<R>(&self, f: impl FnOnce(&[u8]) -> R) -> R {
        f(&self.img)
    }

    /// Mutable image access (test helper).
    pub fn with_image_mut<R>(&mut self, f: impl FnOnce(&mut [u8]) -> R) -> R {
        f(&mut self.img)
    }

    /// Force axis `a` into `Fault` on the next `exchange`.
    ///
    /// This is a latched hard-fault: it stays faulted (the `Fault` statusword
    /// is rewritten every cycle), so the natural `FAULT_RESET` recovery path
    /// in `advance_drive` is not reachable while injected. Call
    /// [`clear_fault`](Self::clear_fault) to lift the injection so the drive
    /// can recover via the normal `CiA` 402 transitions (`FAULT_RESET` ->
    /// `SwitchOnDisabled` -> power-walk -> `OperationEnabled`).
    pub fn inject_fault(&mut self, a: usize) {
        self.faulted[a] = true;
    }

    /// Lift the latched hard-fault on axis `a` (undoes [`inject_fault`]).
    ///
    /// After this, `advance_drive` resumes normal `CiA` 402 state transitions,
    /// so a `FAULT_RESET` controlword walks the drive `Fault` ->
    /// `SwitchOnDisabled`, from which the power machine can re-enable. The
    /// drive's current statusword is left as-is (still `Fault` until the next
    /// `FAULT_RESET` exchange) so recovery follows the real handshake.
    ///
    /// [`inject_fault`]: Self::inject_fault
    pub fn clear_fault(&mut self, a: usize) {
        self.faulted[a] = false;
    }

    /// Make axis `a`'s device stale (drops out of the cycle).
    pub fn inject_stale(&mut self, a: usize, on: bool) {
        self.stale[a] = on;
    }

    fn advance_drive(&mut self, a: usize) {
        // Index bounded by n; no overflow within realistic axis counts.
        #[allow(clippy::cast_possible_truncation)]
        let d = MockDrive::for_axis(a as u16);

        if self.faulted[a] {
            let b = a * BYTES_PER_AXIS;
            // Fault statusword: 0x0208.
            self.img[b + 7..b + 9].copy_from_slice(&0x0208u16.to_le_bytes());
            return;
        }

        let ctrl = u16::from_le_bytes([
            self.img[a * BYTES_PER_AXIS],
            self.img[a * BYTES_PER_AXIS + 1],
        ]);
        let sw = d.statusword(&self.img);
        let next_sw = match (decode_state(sw), ctrl) {
            (Cia402State::SwitchOnDisabled, c) if c == cw::SHUTDOWN => 0x0231, // ReadyToSwitchOn
            (Cia402State::ReadyToSwitchOn, c) if c == cw::SWITCH_ON => 0x0233, // SwitchedOn
            (Cia402State::SwitchedOn, c) if c == cw::ENABLE_OPERATION => 0x0237, // OperationEnabled
            (Cia402State::Fault, c) if c == cw::FAULT_RESET => 0x0250, // back to SwitchOnDisabled
            (s, _) => encode_state(s),
        };

        let b = a * BYTES_PER_AXIS;
        self.img[b + 7..b + 9].copy_from_slice(&next_sw.to_le_bytes());

        // Perfect `CSP` follower: actual := target when `OperationEnabled`.
        if decode_state(next_sw) == Cia402State::OperationEnabled {
            let target = i32::from_le_bytes([
                self.img[b + 3],
                self.img[b + 4],
                self.img[b + 5],
                self.img[b + 6],
            ]);
            self.img[b + 9..b + 13].copy_from_slice(&target.to_le_bytes());
        }
    }
}

const fn encode_state(s: Cia402State) -> u16 {
    match s {
        Cia402State::SwitchOnDisabled => 0x0250,
        Cia402State::ReadyToSwitchOn => 0x0231,
        Cia402State::SwitchedOn => 0x0233,
        Cia402State::OperationEnabled => 0x0237,
        Cia402State::QuickStopActive => 0x0207,
        Cia402State::FaultReactionActive => 0x020F,
        Cia402State::Fault => 0x0208,
        Cia402State::NotReadyToSwitchOn => 0x0000,
    }
}

impl CyclicFieldbus for MockCyclicFieldbus {
    type Routing = MockRouting;
    type Error = core::convert::Infallible;

    // `exchange` is async on the trait because a real bus awaits the cycle
    // phase and the wire round; the virtual drives advance in memory, so the
    // work happens eagerly and an already-resolved future is returned.
    fn exchange(
        &mut self,
    ) -> impl core::future::Future<Output = Result<CycleQuality, Self::Error>> {
        core::future::ready({
            for a in 0..self.n {
                if !self.stale[a] {
                    self.advance_drive(a);
                }
            }
            let all_fresh = !self.stale.iter().any(|s| *s);
            let q = CycleQuality {
                cycle_index: self.cycle,
                all_devices_fresh: all_fresh,
            };
            self.cycle += 1;
            Ok(q)
        })
    }

    fn read_input(&self, r: &MockRouting, dst: &mut [u8]) -> Validity {
        // Index bounded by n; no overflow within realistic axis counts.
        #[allow(clippy::cast_possible_truncation)]
        let axis = r.axis as usize;
        let b = axis * BYTES_PER_AXIS + 7;
        dst[..6].copy_from_slice(&self.img[b..b + 6]); // sw(2) + actual(4)
        if self.stale[axis] {
            Validity::Stale { cycles: 1 }
        } else {
            Validity::Fresh
        }
    }

    fn write_output(&mut self, r: &MockRouting, src: &[u8]) {
        // Index bounded by n; no overflow within realistic axis counts.
        #[allow(clippy::cast_possible_truncation)]
        let b = r.axis as usize * BYTES_PER_AXIS;
        self.img[b..b + 7].copy_from_slice(&src[..7]); // cw(2) + mode(1) + target(4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use taktora_cia402::Cia402Drive;
    use taktora_cia402::state::{Cia402State, decode_state};

    #[test]
    fn virtual_drive_powers_on_and_follows_target() {
        let mut bus = MockCyclicFieldbus::new(1); // 1 axis
        let drive = MockDrive::for_axis(0);
        // Walk the power state machine by writing controlwords until enabled.
        let mut sw = 0;
        for _ in 0..10 {
            let m = taktora_cia402::PowerStateMachine::new(taktora_cia402::PowerTarget::Enabled);
            let cw_out = m.next_controlword(sw);
            bus.with_image_mut(|img| drive.set_controlword(img, cw_out));
            block_on(bus.exchange()).unwrap();
            sw = bus.with_image(|img| drive.statusword(img));
            if decode_state(sw) == Cia402State::OperationEnabled {
                break;
            }
        }
        assert_eq!(decode_state(sw), Cia402State::OperationEnabled);

        // Now command a target; actual should track it next cycle.
        bus.with_image_mut(|img| drive.set_target_position(img, 5000));
        block_on(bus.exchange()).unwrap();
        let actual = bus.with_image(|img| drive.actual_position(img));
        assert_eq!(actual, 5000);
    }

    #[test]
    fn fault_injection_sets_fault_bit_and_stale() {
        let mut bus = MockCyclicFieldbus::new(1);
        bus.inject_fault(0);
        block_on(bus.exchange()).unwrap();
        let drive = MockDrive::for_axis(0);
        assert_eq!(
            taktora_cia402::state::decode_state(bus.with_image(|i| drive.statusword(i))),
            taktora_cia402::state::Cia402State::Fault
        );
        bus.inject_stale(0, true);
        let q = block_on(bus.exchange()).unwrap();
        assert!(!q.all_devices_fresh);
    }

    #[test]
    fn clear_fault_lets_a_faulted_drive_recover() {
        // Change B: while `inject_fault` is latched the drive stays in `Fault`
        // (statusword rewritten every cycle); after `clear_fault` the normal
        // `CiA` 402 transitions resume and the power machine re-enables.
        let mut bus = MockCyclicFieldbus::new(1);
        let drive = MockDrive::for_axis(0);

        // Latch a hard-fault and confirm it sticks across cycles.
        bus.inject_fault(0);
        for _ in 0..4 {
            block_on(bus.exchange()).unwrap();
        }
        assert_eq!(
            decode_state(bus.with_image(|i| drive.statusword(i))),
            Cia402State::Fault,
            "injected fault is latched"
        );

        // Lift the injection, then drive the power machine toward Enabled. The
        // machine emits FAULT_RESET from `Fault`, walking the drive back to
        // SwitchOnDisabled, then through the normal power handshake.
        bus.clear_fault(0);
        let machine = taktora_cia402::PowerStateMachine::new(taktora_cia402::PowerTarget::Enabled);
        let mut sw = bus.with_image(|i| drive.statusword(i));
        for _ in 0..12 {
            let cw_out = machine.next_controlword(sw);
            bus.with_image_mut(|img| drive.set_controlword(img, cw_out));
            block_on(bus.exchange()).unwrap();
            sw = bus.with_image(|i| drive.statusword(i));
            if decode_state(sw) == Cia402State::OperationEnabled {
                break;
            }
        }
        assert_eq!(
            decode_state(sw),
            Cia402State::OperationEnabled,
            "after clear_fault the drive recovers to OperationEnabled"
        );
    }

    fn block_on<F: core::future::Future>(f: F) -> F::Output {
        pollster::block_on(f)
    }
}
