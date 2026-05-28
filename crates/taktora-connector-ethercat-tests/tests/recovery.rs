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

/// TEST_0224 (precursor) — CycleKind::Plain recorded by default.
#[tokio::test]
async fn mock_records_cycle_kind_plain_by_default() {
    use taktora_connector_ethercat::mock::CycleKind;
    let mut mock = MockBusDriver::new();
    mock.cycle().await.expect("cycle");
    assert_eq!(mock.cycle_kinds(), vec![CycleKind::Plain]);
}

/// TEST_0224 — CycleKind::Dc recorded when the mock is marked DC.
#[tokio::test]
async fn mock_records_cycle_kind_dc_when_marked() {
    use taktora_connector_ethercat::mock::CycleKind;
    let mut mock = MockBusDriver::new().with_dc_cycle_kind();
    mock.cycle().await.expect("cycle");
    assert_eq!(mock.cycle_kinds(), vec![CycleKind::Dc]);
}

use std::sync::Arc as TestArc;
use std::time::Duration;

use taktora_connector_core::ExponentialBackoffBuilder;
use taktora_connector_ethercat::{CycleRunner, EthercatHealthMonitor};

/// TEST_0225 — cycle Err triggers BusDriver::recover per policy and
/// the runner adopts the recover()'s expected_wkc.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn cycle_err_triggers_recover_per_policy() {
    let opts = EthercatConnectorOptions::builder()
        .network_interface("mock0")
        .cycle_time(Duration::from_millis(1))
        .reconnect_policy_factory(TestArc::new(|| {
            Box::new(
                ExponentialBackoffBuilder::new()
                    .max_attempts(3)
                    .initial(Duration::from_millis(1))
                    .max(Duration::from_millis(10))
                    .jitter(0.0)
                    .build(),
            )
        }))
        .build();

    let mock = MockBusDriver::new()
        .with_bring_up(BringUp {
            expected_wkc: 3,
            subdevice_count: 1,
        })
        .with_wkc_sequence([3, 3])
        .with_cycle_err_after(3, "synthetic fault")
        .with_recovery_sequence([Ok::<_, String>(BringUp {
            expected_wkc: 5,
            subdevice_count: 1,
        })])
        .with_default_cycle_wkc(5);

    let health = TestArc::new(EthercatHealthMonitor::new());
    let mut runner = CycleRunner::new(mock, &opts, TestArc::clone(&health))
        .await
        .expect("bring_up");

    // Drive a series of ticks. Each tick advances tokio's paused
    // clock so the scheduler's `cycle_time` is honoured.
    let mut now = std::time::Instant::now();
    for _ in 0..8 {
        let _ = runner.tick(now).await;
        now += Duration::from_millis(2);
    }

    assert_eq!(runner.driver().recover_calls(), 1);
    assert_eq!(runner.expected_wkc(), 5);
}

/// TEST_0226 — exact HealthEvent sequence across a single recovery
/// episode.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn health_events_match_recovery_table() {
    let opts = EthercatConnectorOptions::builder()
        .network_interface("mock0")
        .cycle_time(Duration::from_millis(1))
        .reconnect_policy_factory(TestArc::new(|| {
            Box::new(
                ExponentialBackoffBuilder::new()
                    .max_attempts(3)
                    .initial(Duration::from_millis(1))
                    .max(Duration::from_millis(2))
                    .jitter(0.0)
                    .build(),
            )
        }))
        .build();

    let mock = MockBusDriver::new()
        .with_bring_up(BringUp {
            expected_wkc: 3,
            subdevice_count: 1,
        })
        .with_wkc_sequence([3])
        .with_cycle_err_after(2, "synthetic fault")
        .with_recovery_sequence([Ok::<_, String>(BringUp {
            expected_wkc: 3,
            subdevice_count: 1,
        })])
        .with_default_cycle_wkc(3);

    let health = TestArc::new(EthercatHealthMonitor::new());
    let sub = health.subscribe();
    let mut runner = CycleRunner::new(mock, &opts, TestArc::clone(&health))
        .await
        .expect("bring_up");

    let mut now = std::time::Instant::now();
    for _ in 0..6 {
        let _ = runner.tick(now).await;
        now += Duration::from_millis(2);
    }

    // Drain the health-event subscription. `subscribe()` returns a
    // `crossbeam_channel::Receiver`, so the idiomatic non-blocking
    // drain is `try_recv()` until `Empty` is returned. Exact expected
    // sequence: Up (set by bring_up) → Degraded (cycle failed) →
    // Connecting (recover episode in flight) → Up (recover succeeded).
    let mut kinds = vec![];
    while let Ok(ev) = sub.try_recv() {
        kinds.push(ev.to.kind());
    }

    use taktora_connector_core::ConnectorHealthKind::{Connecting, Degraded, Up};
    assert_eq!(kinds, vec![Up, Degraded, Connecting, Up]);
}
