//! Hardware-gated `EthercrabBusDriver` integration test. Gated on
//! the `bus-integration` cargo feature; marked `#[ignore]` so it does
//! not run during normal `cargo test` invocations.
//!
//! ## Running against real hardware
//!
//! ```sh
//! # Linux gateway host, network interface `eth0`, requires CAP_NET_RAW
//! ETHERCAT_TEST_NIC=eth0 \
//!   cargo test -p taktora-connector-ethercat \
//!     --features bus-integration \
//!     --test ethercrab_driver \
//!     -- --ignored --test-threads=1
//! ```
//!
//! Tests that require `ETHERCAT_TEST_NIC` shall return early with a
//! clear log message when the env var is absent so the test runner
//! doesn't surface them as failures during CI or local-dev runs that
//! happen to enable the `bus-integration` feature.

#![cfg(feature = "bus-integration")]
#![allow(clippy::doc_markdown)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use taktora_connector_core::ConnectorHealthKind;
use taktora_connector_ethercat::bus::EthercatPduStorage;
use taktora_connector_ethercat::driver::BusDriver;
use taktora_connector_ethercat::{
    CycleRunner, EthercatConnectorOptions, EthercatHealthMonitor, EthercrabBusDriver, SmWatchdog,
    SubDeviceMap, declare_pdu_storage,
};

declare_pdu_storage!(TEST_PDU_STORAGE);

const MAX_SUBDEVICES: usize = 16;
const MAX_PDI: usize = 256;

fn maybe_nic() -> Option<String> {
    let iface = std::env::var("ETHERCAT_TEST_NIC").ok()?;
    if iface.trim().is_empty() {
        None
    } else {
        Some(iface)
    }
}

/// Bus comes up to OP, cycle returns a working counter, health
/// transitions Connecting → Up. Requires a Linux host with an actual
/// EtherCAT bus on `ETHERCAT_TEST_NIC`.
#[tokio::test]
#[ignore = "requires real EtherCAT NIC; set ETHERCAT_TEST_NIC=<iface> and run with --ignored"]
async fn bring_up_and_cycle_against_real_bus() {
    let Some(iface) = maybe_nic() else {
        eprintln!("ETHERCAT_TEST_NIC not set; skipping");
        return;
    };

    let options = EthercatConnectorOptions::builder()
        .network_interface(iface)
        .cycle_time(Duration::from_millis(2))
        .build();

    let driver =
        EthercrabBusDriver::<MAX_SUBDEVICES, MAX_PDI>::new(&TEST_PDU_STORAGE, options.clone())
            .expect("driver construction");
    let health = Arc::new(EthercatHealthMonitor::new());

    // Box::pin because CycleRunner::new captures the full bring-up
    // future, which carries a sizeable SubDeviceGroup; pinning it
    // on the heap keeps the stack frame manageable.
    let mut runner = Box::pin(CycleRunner::new(driver, &options, Arc::clone(&health)))
        .await
        .expect("bring-up succeeds");

    assert_eq!(
        health.current().kind(),
        ConnectorHealthKind::Up,
        "bring-up should transition Connecting → Up"
    );

    // Run a handful of cycles; each tick should fire (cycle_time
    // elapsed between iterations) and return SOME working counter
    // ≥ 0. A more thorough test would assert specific WKC values
    // against known SubDevice topology.
    let mut now = Instant::now();
    for _ in 0..5 {
        // Box::pin to keep the large CycleRunner::tick future off the
        // stack (clippy::large_futures — the future carries the full
        // SubDeviceGroup + scheduler state).
        let report = Box::pin(runner.tick(now))
            .await
            .expect("cycle succeeds")
            .expect("scheduler fires");
        eprintln!(
            "cycle {}: WKC={}",
            report.cycle_index, report.working_counter
        );
        now += options.cycle_time();
    }
}

/// REQ_0846 / AOU_0016 — bring-up programs the SM-watchdog registers
/// (`0x0400` / `0x0420`) and verifies them by read-back. Requires both
/// a real NIC (`ETHERCAT_TEST_NIC`) and the configured address of an
/// output SubDevice (`ETHERCAT_TEST_WD_ADDRESS`, hex e.g. `0x1002`):
/// the driver writes a 50 ms window and read-back-fails the bring-up if
/// the registers do not stick, so reaching OP here is the hardware
/// evidence that the write + verify path applies.
#[tokio::test]
#[ignore = "requires real EtherCAT NIC + ETHERCAT_TEST_WD_ADDRESS; run with --ignored"]
async fn bring_up_programs_and_verifies_sm_watchdog() {
    // `static` at the top of the function so the clippy
    // `items_after_statements` lint stays clean.
    static WD_MAP: std::sync::OnceLock<Vec<SubDeviceMap>> = std::sync::OnceLock::new();

    let Some(iface) = maybe_nic() else {
        eprintln!("ETHERCAT_TEST_NIC not set; skipping");
        return;
    };
    let Some(address) = std::env::var("ETHERCAT_TEST_WD_ADDRESS")
        .ok()
        .and_then(|s| u16::from_str_radix(s.trim_start_matches("0x"), 16).ok())
    else {
        eprintln!("ETHERCAT_TEST_WD_ADDRESS not set; skipping");
        return;
    };

    // 50 ms = FTTI/2 = 500 ticks of the default 100 µs divider.
    let map: &'static [SubDeviceMap] = WD_MAP.get_or_init(|| {
        vec![
            SubDeviceMap::new(address, &[], &[], 0)
                .with_sm_watchdog(SmWatchdog::from_timeout_us(50_000)),
        ]
    });

    let options = EthercatConnectorOptions::builder()
        .network_interface(iface)
        .pdo_map(map)
        .build();

    let mut driver = EthercrabBusDriver::<MAX_SUBDEVICES, MAX_PDI>::new(&TEST_PDU_STORAGE, options)
        .expect("driver construction");

    // bring_up internally writes 0x0400/0x0420 and read-back-verifies
    // them; a mismatch returns Err, so `expect` here is the assertion.
    let report = Box::pin(driver.bring_up())
        .await
        .expect("bring-up programs and verifies the SM watchdog");
    assert!(report.subdevice_count > 0);
}

#[ignore = "requires CAP_NET_RAW + EtherCAT NIC; set ETHERCAT_TEST_NIC"]
#[tokio::test]
async fn recover_returns_to_op_without_storage_resplit() {
    // `static` declared at the top of the function so the clippy
    // `items_after_statements` lint stays clean.
    static STORAGE: EthercatPduStorage = EthercatPduStorage::new();

    let interface = std::env::var("ETHERCAT_TEST_NIC")
        .expect("ETHERCAT_TEST_NIC environment variable required");

    let opts = EthercatConnectorOptions::builder()
        .network_interface(&interface)
        .build();
    let mut driver: EthercrabBusDriver<16, 64> =
        EthercrabBusDriver::new(&STORAGE, opts).expect("construct driver");

    // Box::pin to keep the large bring_up / recover futures off the
    // stack (clippy::large_futures — the futures carry the full
    // SubDeviceGroup + PduStorage borrows).
    let bring_up = Box::pin(driver.bring_up()).await.expect("bring_up");
    assert!(bring_up.subdevice_count > 0);

    let recover = Box::pin(driver.recover()).await.expect("recover");
    assert_eq!(recover.subdevice_count, bring_up.subdevice_count);
}
