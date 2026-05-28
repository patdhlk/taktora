//! Tests for the bus-driver finishing slice — asymmetric WKC,
//! BusDriver::recover, CycleKind DC branch, recovery state machine.

use taktora_connector_ethercat::{EthercatConnectorOptions, PdoEntry, SubDeviceMap};

const PDO_ENTRY_TX: PdoEntry = PdoEntry {
    index: 0x1A00,
    bit_offset: 0,
    bit_length: 8,
};
const PDO_ENTRY_RX: PdoEntry = PdoEntry {
    index: 0x1600,
    bit_offset: 0,
    bit_length: 8,
};

/// TEST_0223 — asymmetric expected_wkc summing.
#[test]
fn expected_wkc_sums_per_subdevicemap() {
    static MAP: &[SubDeviceMap] = &[
        SubDeviceMap {
            address: 0x1000, // EK1100 coupler — 0
            rx_pdos: &[],
            tx_pdos: &[],
            expected_wkc: 0,
        },
        SubDeviceMap {
            address: 0x1001, // EL1008 inputs — 2
            rx_pdos: &[],
            tx_pdos: &[PDO_ENTRY_TX],
            expected_wkc: 2,
        },
        SubDeviceMap {
            address: 0x1002, // EL2004 outputs — 1
            rx_pdos: &[PDO_ENTRY_RX],
            tx_pdos: &[],
            expected_wkc: 1,
        },
    ];
    let opts = EthercatConnectorOptions::builder()
        .network_interface("mock0")
        .pdo_map(MAP)
        .build();

    let sum = taktora_connector_ethercat::expected_wkc_from_map(&opts);
    assert_eq!(sum, 3);
}
