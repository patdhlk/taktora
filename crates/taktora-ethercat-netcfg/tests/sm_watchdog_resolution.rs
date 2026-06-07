//! Resolution + arithmetic for the per-device SM-watchdog (`REQ_0844`,
//! `TEST_0860`). Every device carrying output (rx) PDOs resolves an
//! effective watchdog timeout — the per-device `sm_watchdog_timeout_ms`
//! override if present, else FTTI/2 — and quantizes it to ESC register
//! values using the SAME arithmetic as the connector's `SmWatchdog`
//! (divider 2498 → 100 µs ticks, intervals = ceil(timeout_us / 100),
//! clamped to `1..=u16::MAX`). Cross-reference:
//! `crates/taktora-connector-ethercat/src/watchdog.rs`.
//!
//! The doc comments here spell out tick arithmetic (`ceil`, `50_000 µs`,
//! …) as prose, so `doc_markdown` is silenced file-wide.
#![allow(clippy::doc_markdown)]

use core::time::Duration;

use taktora_ethercat_netcfg::{DeviceInstance, SmWatchdogRegisters, parse};

/// Locate a device by label in a parsed config.
fn device<'a>(cfg: &'a taktora_ethercat_netcfg::NetworkConfig, label: &str) -> &'a DeviceInstance {
    cfg.devices
        .iter()
        .find(|d| d.label == label)
        .unwrap_or_else(|| panic!("device `{label}` present"))
}

/// An rx-carrying device with no override resolves to FTTI/2.
///
/// Default FTTI is 100 ms, so the bound is 50 ms = 50_000 µs.
/// `intervals = ceil(50_000 / 100) = 500`, divider = 2498.
#[test]
fn rx_device_default_ftti_half() {
    let yaml = r"
schema_version: 1
bus:
  cycle_time_ms: 2
  distributed_clocks: false
  max_subdevices: 16
  max_pdi_bytes: 256
devices:
  - label: outputs
    sm_watchdog_enabled: true
    pdos:
      rx: [{ index: 0x7000, bit_offset: 0, bit_length: 8 }]
channels: []
";
    let cfg = parse(yaml).expect("default-FTTI rx device parses");
    assert_eq!(cfg.bus.ftti, Duration::from_millis(100));

    let dev = device(&cfg, "outputs");
    assert_eq!(
        dev.sm_watchdog,
        Some(SmWatchdogRegisters {
            divider: 2498,
            intervals: 500,
        }),
        "FTTI/2 = 50 ms resolves to 500 ticks of 100 µs"
    );
}

/// A per-device override below the bound resolves to its own value.
///
/// `sm_watchdog_timeout_ms: 10` → 10_000 µs → ceil(10_000/100) = 100 ticks.
#[test]
fn rx_device_override_below_bound() {
    let yaml = r"
schema_version: 1
bus:
  cycle_time_ms: 2
  distributed_clocks: false
  max_subdevices: 16
  max_pdi_bytes: 256
devices:
  - label: outputs
    sm_watchdog_enabled: true
    sm_watchdog_timeout_ms: 10
    pdos:
      rx: [{ index: 0x7000, bit_offset: 0, bit_length: 8 }]
channels: []
";
    let cfg = parse(yaml).expect("override rx device parses");
    let dev = device(&cfg, "outputs");
    assert_eq!(
        dev.sm_watchdog,
        Some(SmWatchdogRegisters {
            divider: 2498,
            intervals: 100,
        }),
    );
}

/// A custom FTTI feeds FTTI/2 when there is no override. FTTI 40 ms →
/// bound 20 ms = 20_000 µs → 200 ticks.
#[test]
fn rx_device_custom_ftti() {
    let yaml = r"
schema_version: 1
bus:
  cycle_time_ms: 2
  distributed_clocks: false
  max_subdevices: 16
  max_pdi_bytes: 256
  ftti_ms: 40
devices:
  - label: outputs
    sm_watchdog_enabled: true
    pdos:
      rx: [{ index: 0x7000, bit_offset: 0, bit_length: 8 }]
channels: []
";
    let cfg = parse(yaml).expect("custom-FTTI rx device parses");
    assert_eq!(cfg.bus.ftti, Duration::from_millis(40));
    let dev = device(&cfg, "outputs");
    assert_eq!(
        dev.sm_watchdog,
        Some(SmWatchdogRegisters {
            divider: 2498,
            intervals: 200,
        }),
    );
}

/// An input-only device (tx PDOs only, no rx) resolves NO watchdog.
#[test]
fn input_only_device_resolves_no_watchdog() {
    let yaml = r"
schema_version: 1
bus:
  cycle_time_ms: 2
  distributed_clocks: false
  max_subdevices: 16
  max_pdi_bytes: 256
devices:
  - label: inputs
    pdos:
      tx: [{ index: 0x6000, bit_offset: 0, bit_length: 8 }]
channels: []
";
    let cfg = parse(yaml).expect("input-only device parses");
    let dev = device(&cfg, "inputs");
    assert_eq!(
        dev.sm_watchdog, None,
        "input-only device carries no watchdog"
    );
}

/// The netcfg quantization arithmetic must agree, value-for-value, with
/// the connector's documented `SmWatchdog::from_timeout_us` /
/// `effective_timeout_ns` semantics (deliberately duplicated; see
/// `crates/taktora-connector-ethercat/src/watchdog.rs`). This pins the
/// representative values the connector's own tests assert.
#[test]
fn arithmetic_matches_connector_semantics() {
    // (timeout_us, expected_intervals): ceil(timeout_us / 100), clamped.
    let cases = [
        (0u32, 1u16),  // clamps up to one tick, never a disabled 0
        (1, 1),        // 1 µs rounds up to one 100 µs tick
        (100, 1),      // exact multiple
        (150, 2),      // rounds up
        (50_000, 500), // FTTI/2 default
        (10_000, 100), // 10 ms override
    ];
    for (timeout_us, expected_intervals) in cases {
        let intervals = taktora_ethercat_netcfg::sm_watchdog_intervals(timeout_us);
        assert_eq!(
            intervals, expected_intervals,
            "timeout {timeout_us} µs → {expected_intervals} ticks"
        );
        // effective window = 40 ns × (2498 + 2) × intervals = 100 µs × intervals.
        let effective_ns = 40u64 * (2498 + 2) * u64::from(intervals);
        assert_eq!(effective_ns, 100_000 * u64::from(intervals));
    }
}
