//! Tests for the bus-driver finishing slice — asymmetric WKC,
//! BusDriver::recover, CycleKind DC branch, recovery state machine.

use taktora_connector_ethercat::MockBusDriver;
use taktora_connector_ethercat::driver::{BringUp, BusDriver};
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

/// TEST_0225 (precursor) — MockBusDriver::recover returns programmed
/// outcomes in order.
#[tokio::test]
async fn mock_recover_returns_programmed_sequence() {
    let mut mock = MockBusDriver::new().with_recovery_sequence([
        Err("transient fault"),
        Ok(BringUp {
            expected_wkc: 7,
            subdevice_count: 3,
        }),
    ]);

    // First recover attempt: programmed Err.
    let r1 = mock.recover().await;
    assert!(r1.is_err(), "first recover should fail per program");

    // Second: programmed Ok.
    let r2 = mock.recover().await.expect("second recover succeeds");
    assert_eq!(r2.expected_wkc, 7);
    assert_eq!(r2.subdevice_count, 3);

    assert_eq!(mock.recover_calls(), 2);
}

/// TEST_0225 (precursor) — options expose a reconnect policy factory
/// with a sensible default.
#[test]
fn options_default_reconnect_policy_factory_yields_fresh_instances() {
    let opts = EthercatConnectorOptions::builder()
        .network_interface("mock0")
        .build();
    // Calling new_reconnect_policy should yield a fresh boxed-dyn
    // instance on every call.
    let p1 = opts.new_reconnect_policy();
    let p2 = opts.new_reconnect_policy();
    // The two box pointers must differ — they are independent
    // allocations.
    assert!(
        !std::ptr::eq(
            &*p1 as *const dyn taktora_connector_core::ReconnectPolicy as *const (),
            &*p2 as *const dyn taktora_connector_core::ReconnectPolicy as *const (),
        ),
        "factory must yield a fresh boxed-dyn per call"
    );
}
