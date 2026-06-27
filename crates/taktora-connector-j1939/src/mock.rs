//! [`MockJ1939Interface`] — layer-1 test harness over
//! `taktora_connector_can::MockCanInterface`. `BB_0103`, `REQ_0899`.
//!
//! Ships **ungated** (no `socketcan-integration`, no kernel CAN module)
//! so the layer-1 test pyramid exercises PGN routing — and, later, the
//! TP and address-claim machines — deterministically on any host OS.
//!
//! ## Shape
//!
//! `MockJ1939Interface` *owns* a [`MockCanInterface`] until a test (or
//! the connector) takes it via `into_driver` to hand to the
//! dispatcher. Independently it exposes:
//!
//! * `inject_j1939` — encode a 29-bit id from
//!   `(pgn, priority, sa, da)` and inject a single classical
//!   `CanFrame::Data` as if it arrived from the bus. This is what
//!   TEST_0886 / TEST_0895 drive.
//! * `inject_raw_frame` — inject a pre-built [`CanData`] (for
//!   tests that craft the id by hand).
//! * `tx_handle` — the underlying mpsc sender, for tests that
//!   want to push arbitrary [`CanFrame`]s (error frames, remote frames,
//!   and — for #123 onward — multi-frame TP sequences).
//!
//! ## For #123 / #124 / #125 / #126
//!
//! The injection side is deliberately generic: BAM / RTS-CTS / ETP
//! tests inject the TP.CM and TP.DT PGNs through `inject_j1939`
//! in sequence; address-claim tests (#126) inject PGN 60928 frames the
//! same way. Observation of *reassembled* payloads and claim
//! transitions happens on the iceoryx2 reader / health subscription the
//! connector hands back — this harness only has to get raw frames onto
//! (and off, via loopback `send`) the mock bus.

use taktora_connector_can::{
    CanData, CanFdFlags, CanFrame, CanFrameKind, CanId, CanIface, MockCanInterface,
};
use tokio::sync::mpsc::UnboundedSender;

use crate::decode::encode_extended_id;
use crate::routing::Pgn;

/// In-process J1939 layer-1 harness wrapping a [`MockCanInterface`].
#[derive(Debug)]
pub struct MockJ1939Interface {
    iface: CanIface,
    driver: MockCanInterface,
    tx: UnboundedSender<CanFrame>,
}

impl MockJ1939Interface {
    /// Construct a fresh harness bound to `iface`.
    #[must_use]
    pub fn new(iface: CanIface) -> Self {
        let driver = MockCanInterface::new(iface);
        let tx = driver.tx_handle();
        Self { iface, driver, tx }
    }

    /// Borrow the bound interface.
    #[must_use]
    pub const fn iface(&self) -> &CanIface {
        &self.iface
    }

    /// Borrow the underlying mock driver (e.g. to inspect filter /
    /// send-count state).
    #[must_use]
    pub const fn driver(&self) -> &MockCanInterface {
        &self.driver
    }

    /// Mutably borrow the underlying mock driver (e.g. to drive
    /// `dispatch_one_iteration` directly while keeping the harness for
    /// injection).
    pub const fn driver_mut(&mut self) -> &mut MockCanInterface {
        &mut self.driver
    }

    /// Consume the harness, yielding the owned [`MockCanInterface`] to
    /// hand to a dispatcher task. Inject frames *before* calling this,
    /// or clone a `tx_handle` first.
    #[must_use]
    pub fn into_driver(self) -> MockCanInterface {
        self.driver
    }

    /// Clone of the internal sender so tests can push arbitrary
    /// [`CanFrame`]s after `into_driver` has moved the driver.
    #[must_use]
    pub fn tx_handle(&self) -> UnboundedSender<CanFrame> {
        self.tx.clone()
    }

    /// Encode a 29-bit id from `(pgn, priority, source_addr, dest_addr)`
    /// and inject a single classical-CAN data frame carrying `payload`
    /// as if it arrived from the bus.
    ///
    /// `payload` must be `<= 8` bytes (single-frame). Returns the
    /// encoded raw identifier so tests can assert on it.
    ///
    /// # Errors
    ///
    /// Returns the underlying [`taktora_connector_can::CanIoError`] when
    /// `payload` exceeds 8 bytes or the encoded id is out of range.
    pub fn inject_j1939(
        &self,
        pgn: Pgn,
        priority: u8,
        source_addr: u8,
        dest_addr: Option<u8>,
        payload: &[u8],
    ) -> Result<u32, taktora_connector_can::CanIoError> {
        let raw = encode_extended_id(pgn, priority, source_addr, dest_addr);
        let can_id = CanId::extended(raw)
            .map_err(|e| taktora_connector_can::CanIoError::Io(format!("encode id: {e}")))?;
        let data = CanData::new(
            can_id,
            CanFrameKind::Classical,
            CanFdFlags::empty(),
            payload,
        )?;
        self.inject_raw_frame(data);
        Ok(raw)
    }

    /// Inject a pre-built [`CanData`] as an inbound bus frame.
    pub fn inject_raw_frame(&self, data: CanData) {
        let _ = self.tx.send(CanFrame::Data(data));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inject_j1939_encodes_expected_id() {
        let h = MockJ1939Interface::new(CanIface::new("vcan0").unwrap());
        // Request PGN 59904, priority 6, SA 0x11, DA 0x21.
        let raw = h
            .inject_j1939(Pgn::new(59904).unwrap(), 6, 0x11, Some(0x21), &[1, 2, 3])
            .unwrap();
        assert_eq!(raw, 0x18EA_2111);
    }
}
