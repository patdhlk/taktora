//! `TEST_0862` — pure arithmetic of the SM-watchdog register model.
//!
//! Exercises [`SmWatchdog`] tick maths against the AOU_0016 bound
//! (output-slave SM watchdog ≤ FTTI/2 = 50 ms) without bus hardware.
//! The register write/read-back path is hardware-gated and lives in
//! the `#[ignore]`-gated `tests/ethercrab_driver.rs`.

// Bare type names (u16::MAX) and EtherCAT terms appear in these doc
// comments; mirror the crate's own `doc_markdown` posture.
#![allow(clippy::doc_markdown)]

use taktora_connector_ethercat::SmWatchdog;
use taktora_connector_ethercat::watchdog::DEFAULT_DIVIDER;

/// ETG tick maths: with the default divider 2498, one watchdog tick is
/// 40 ns × (2498 + 2) = 100 µs. `intervals` ticks of 100 µs each.
#[test]
fn default_divider_tick_is_100_microseconds() {
    let one_tick = SmWatchdog {
        divider: DEFAULT_DIVIDER,
        intervals: 1,
    };
    assert_eq!(
        one_tick.effective_timeout_ns(),
        100_000,
        "one tick = 100 µs"
    );
    assert_eq!(DEFAULT_DIVIDER, 2498, "ESC default 100 µs divider");
}

/// 50 ms is exactly 500 ticks of 100 µs — the AOU_0016 bound lands on a
/// tick boundary, so `from_timeout_us(50_000)` is exact (no over-shoot).
#[test]
fn fifty_ms_is_exactly_500_ticks() {
    let wd = SmWatchdog::from_timeout_us(50_000);
    assert_eq!(wd.divider, DEFAULT_DIVIDER);
    assert_eq!(wd.intervals, 500, "50 ms / 100 µs = 500 ticks");
    assert_eq!(
        wd.effective_timeout_ns(),
        50_000_000,
        "effective timeout is exactly 50 ms"
    );
}

/// Ceil quantization: 50_001 µs needs 501 ticks (500 ticks = 50 ms is
/// short), pushing the effective timeout ABOVE 50 ms. The bound that
/// callers must check against is the EFFECTIVE value, not the request.
#[test]
fn ceil_quantization_rounds_up_past_the_bound() {
    let wd = SmWatchdog::from_timeout_us(50_001);
    assert_eq!(wd.intervals, 501, "ceil(50_001 / 100) = 501 ticks");
    assert_eq!(
        wd.effective_timeout_ns(),
        50_100_000,
        "501 ticks = 50.1 ms — above the 50 ms FTTI/2 bound"
    );
    assert!(
        wd.effective_timeout_ns() > 50_000_000,
        "ceil overshoots the requested 50.001 ms to the next tick"
    );
}

/// Sub-tick requests still arm a watchdog: 0 µs clamps to 1 tick rather
/// than the disabling 0-interval value (a disabled watchdog violates
/// AOU_0016 — never emit it from this helper).
#[test]
fn zero_timeout_clamps_to_one_tick() {
    let wd = SmWatchdog::from_timeout_us(0);
    assert_eq!(wd.intervals, 1, "0 µs clamps up to 1 tick, never 0");
    assert_eq!(wd.effective_timeout_ns(), 100_000, "1 tick = 100 µs");
}

/// A request just over one tick still rounds up to 2 ticks.
#[test]
fn one_microsecond_over_a_tick_is_two_ticks() {
    let wd = SmWatchdog::from_timeout_us(101);
    assert_eq!(wd.intervals, 2, "ceil(101 / 100) = 2 ticks");
}

/// Enormous requests saturate at u16::MAX intervals rather than wrap.
#[test]
fn large_timeout_clamps_to_u16_max() {
    let wd = SmWatchdog::from_timeout_us(u32::MAX);
    assert_eq!(wd.intervals, u16::MAX, "saturates at u16::MAX ticks");
    // 40 ns × 2500 × 65535 — no overflow in u64.
    assert_eq!(wd.effective_timeout_ns(), 40 * 2500 * u64::from(u16::MAX));
}

/// `SmWatchdog` is a Copy POD usable in const context.
#[test]
fn is_const_constructible() {
    const WD: SmWatchdog = SmWatchdog::from_timeout_us(50_000);
    const NS: u64 = WD.effective_timeout_ns();
    assert_eq!(NS, 50_000_000);
}
