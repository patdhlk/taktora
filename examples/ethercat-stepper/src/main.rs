//! Integration example: executor + ethercat connector driving a Beckhoff
//! EL7047 stepper terminal off EL1008 button presses, with the EL2004 as a
//! drive-status lamp. See README.md for hardware setup and run instructions.
//!
//! Motion uses the Beckhoff **positioning interface** (NOT CiA 402): each
//! index press fires a relative move whose trapezoid the terminal runs itself.
//! Enable = STM Control bit0; move = Target/Velocity/Start-type in the POS
//! Control PDO plus an Execute edge. The EL7047 process image is hand-written
//! and host-unit-tested in `el7047.rs` because the ESI codegen cannot model
//! this device's selectable PDO assignments; EL1008/EL2004 stay codegen-typed.
//!
//! Topology assumption: `ethercrab` assigns configured station addresses
//! starting at `0x1000` — EK1100 = `0x1000` (bus coupler, no PDI), then
//! EL1008 = `0x1001`, EL2004 = `0x1002`, EL7047 = `0x1003`. Adjust the
//! `SUBDEV_*` constants below if your topology differs.

use core::time::Duration;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Instant;

use bitvec::view::BitView;
use clap::Parser;
use taktora_connector_core::ChannelDescriptor;
use taktora_connector_ethercat::{
    EthercatConnector, EthercatConnectorOptions, EthercatRouting, EthercrabBusDriver, PdoDirection,
    PdoEntry, SdoValue, SmWatchdog, StartupSdo, SubDeviceMap, connector::EthercatState,
    declare_pdu_storage,
};
use taktora_connector_host::Connector;
use taktora_ethercat_esi_rt::{EsiDevice, Lsb0};
use taktora_executor::{ControlFlow, ExecuteResult, Executor, ExecutorError, item_with_triggers};

mod codec;
mod control;
mod el7047;

use codec::RawImageCodec;
use control::{Controller, MoveParams};

/// ESI-generated typed device drivers. `build.rs` runs
/// `taktora-ethercat-esi-build` over `esi/*.xml` and writes
/// `$OUT_DIR/devices.rs`; this module `include!`s it. The `allow`s mirror the
/// codegen landing-pad crate — generated code is not held to this binary's
/// lint bar.
#[allow(
    missing_docs,
    non_camel_case_types,
    dead_code,
    clippy::all,
    clippy::pedantic,
    clippy::nursery
)]
mod generated {
    include!(concat!(env!("OUT_DIR"), "/devices.rs"));
}

/// Channel capacity (iceoryx2 service buffer slots).
const N: usize = 256;

/// EL1008 (digital inputs) configured station address.
const SUBDEV_EL1008: u16 = 0x1001;
/// EL2004 (digital outputs / status lamp) configured station address.
const SUBDEV_EL2004: u16 = 0x1002;
/// EL7047 (stepper) configured station address. Assumes the EL7047 sits last
/// on the bus — VERIFY on the Pi; if wrong, every routing/PDO_MAP address is
/// off.
const SUBDEV_EL7047: u16 = 0x1003;

/// EL1008: 8 digital input bits, one PDI byte.
const ROUTING_BITS_EL1008: u16 = 8;
/// EL2004: 4 digital output bits, one PDI byte.
const ROUTING_BITS_EL2004: u16 = 4;
/// EL7047 positioning-interface output image: 22 bytes = 176 bits.
const ROUTING_BITS_EL7047_OUT: u16 = (el7047::OUTPUT_LEN * 8) as u16;
/// EL7047 positioning-interface input image: 24 bytes = 192 bits.
const ROUTING_BITS_EL7047_IN: u16 = (el7047::INPUT_LEN * 8) as u16;

// EL7047 "Positioning interface" PDO assignment. `pdo_sdo_writes` reads only
// `entry.index` for the `0x1C12`/`0x1C13` SM assignment, so bit_offset and
// bit_length are left at 0 here.
static EL7047_RX: &[PdoEntry] = &[
    PdoEntry {
        index: 0x1601,
        bit_offset: 0,
        bit_length: 0,
    }, // ENC Control (unused)
    PdoEntry {
        index: 0x1602,
        bit_offset: 0,
        bit_length: 0,
    }, // STM Control
    PdoEntry {
        index: 0x1606,
        bit_offset: 0,
        bit_length: 0,
    }, // POS Control
];
static EL7047_TX: &[PdoEntry] = &[
    PdoEntry {
        index: 0x1a01,
        bit_offset: 0,
        bit_length: 0,
    }, // ENC Status (unused)
    PdoEntry {
        index: 0x1a03,
        bit_offset: 0,
        bit_length: 0,
    }, // STM Status
    PdoEntry {
        index: 0x1a07,
        bit_offset: 0,
        bit_length: 0,
    }, // POS Status
];

/// Operator-declared startup SDOs: stepper motor current limits, written in
/// PRE-OP before PDO assignment (REQ_0853). Units = mA. Tune for your motor;
/// see README.md.
static EL7047_STARTUP: &[StartupSdo] = &[
    StartupSdo {
        index: 0x8010,
        subindex: 0x01,
        value: SdoValue::U16(1800),
    }, // max current
    StartupSdo {
        index: 0x8010,
        subindex: 0x02,
        value: SdoValue::U16(900),
    }, // standby current
];

/// Working-counter mapping for WKC-based health (`REQ_0329`). EL1008 (input)
/// = +2, EL2004 (output) = +1, EL7047 (read+write) = +3 (best-guess; verify
/// observed WKC on the Pi). The EL2004 and EL7047 are output-bearing, so per
/// AOU_0016 their SM watchdog is enabled with a 50 ms timeout (≤ FTTI/2) so
/// they drop to safe state within that window on a master stop.
static PDO_MAP: &[SubDeviceMap] = &[
    SubDeviceMap::new(SUBDEV_EL1008, &[], &[], 2),
    SubDeviceMap::new(SUBDEV_EL2004, &[], &[], 1)
        .with_sm_watchdog(SmWatchdog::from_timeout_us(50_000)),
    SubDeviceMap::new(SUBDEV_EL7047, EL7047_RX, EL7047_TX, 3)
        .with_sm_watchdog(SmWatchdog::from_timeout_us(50_000))
        .with_startup_sdos(EL7047_STARTUP),
];

/// Control-loop cadence.
const CONTROL_PERIOD: Duration = Duration::from_millis(10);
/// Lamp/health cadence.
const LAMP_PERIOD: Duration = Duration::from_millis(250);

/// Bus topology bounds passed to `EthercrabBusDriver`.
const MAX_SUBDEVICES: usize = 16;
const MAX_PDI: usize = 256;

declare_pdu_storage!(EXAMPLE_PDU_STORAGE);

#[derive(Parser, Debug)]
#[command(
    name = "ethercat-stepper",
    about = "executor + ethercat connector driving a Beckhoff EL7047 stepper off EL1008 inputs"
)]
struct Cli {
    /// Network interface the EK1100 is wired to (e.g. `eth0`).
    #[arg(long, default_value = "eth0")]
    nic: String,
    /// Increments commanded per index press (relative move delta).
    #[arg(long, default_value_t = 3200)]
    step: i32,
    /// Move velocity (POS-interface raw units).
    #[arg(long, default_value_t = 1000)]
    velocity: i16,
    /// Acceleration ramp (POS-interface raw units).
    #[arg(long, default_value_t = 1000)]
    accel: u16,
    /// Deceleration ramp (POS-interface raw units).
    #[arg(long, default_value_t = 1000)]
    decel: u16,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let params = MoveParams {
        step: cli.step,
        velocity: cli.velocity,
        acceleration: cli.accel,
        deceleration: cli.decel,
    };

    // 1. Options.
    let opts = EthercatConnectorOptions::builder()
        .network_interface(&cli.nic)
        .cycle_time(Duration::from_millis(2))
        .pdo_map(PDO_MAP)
        .build();

    // 2. Driver. EthercrabBusDriver wraps `ethercrab::MainDevice` behind the
    //    `bus-integration` cargo feature; PDU storage is the static above.
    let driver =
        EthercrabBusDriver::<MAX_SUBDEVICES, MAX_PDI>::new(&EXAMPLE_PDU_STORAGE, opts.clone())?;

    // 3. Connector. ONE codec for every channel: RawImageCodec round-trips a
    //    fixed-size `[u8; LEN]` image verbatim. EL1008/EL2004 ride it as
    //    `[u8; 1]`; the EL7047 as its full positioning-interface image.
    let state = Arc::new(EthercatState::new(opts.clone()));
    let mut connector = EthercatConnector::new(state, driver, RawImageCodec)?;

    // 4. Routing + channels.
    //    EL1008 inputs: 8 bits @ 0x1001 (Tx — SubDevice writes, master reads).
    let el1008_routing =
        EthercatRouting::new(SUBDEV_EL1008, PdoDirection::Tx, 0, ROUTING_BITS_EL1008);
    let el1008_desc =
        ChannelDescriptor::<EthercatRouting, N>::new("ethercat.el1008.inputs", el1008_routing)?;
    let reader_el1008 = connector.create_reader::<[u8; 1], N>(&el1008_desc)?;

    //    EL2004 outputs: 4 bits @ 0x1002 (Rx — master writes).
    let el2004_routing =
        EthercatRouting::new(SUBDEV_EL2004, PdoDirection::Rx, 0, ROUTING_BITS_EL2004);
    let el2004_desc =
        ChannelDescriptor::<EthercatRouting, N>::new("ethercat.el2004.outputs", el2004_routing)?;
    let writer_el2004 = connector.create_writer::<[u8; 1], N>(&el2004_desc)?;

    //    EL7047 output image @ 0x1003 (Rx), bit offset 0, 176 bits.
    let el7047_out_routing =
        EthercatRouting::new(SUBDEV_EL7047, PdoDirection::Rx, 0, ROUTING_BITS_EL7047_OUT);
    let el7047_out_desc = ChannelDescriptor::<EthercatRouting, N>::new(
        "ethercat.el7047.control",
        el7047_out_routing,
    )?;
    let writer_el7047 = connector.create_writer::<[u8; el7047::OUTPUT_LEN], N>(&el7047_out_desc)?;

    //    EL7047 input image @ 0x1003 (Tx), bit offset 0, 192 bits.
    let el7047_in_routing =
        EthercatRouting::new(SUBDEV_EL7047, PdoDirection::Tx, 0, ROUTING_BITS_EL7047_IN);
    let el7047_in_desc =
        ChannelDescriptor::<EthercatRouting, N>::new("ethercat.el7047.status", el7047_in_routing)?;
    let reader_el7047 = connector.create_reader::<[u8; el7047::INPUT_LEN], N>(&el7047_in_desc)?;

    // 5. Executor.
    let mut exec = Executor::builder().worker_threads(1).build()?;
    connector.register_with(&mut exec)?;

    // Shared connector-health flag, set by the health pump and read by the
    // control loop. `true` = Up; Degraded/Down = `false` (software safe-state).
    let healthy = Arc::new(AtomicBool::new(false));
    let started_at = Instant::now();

    // 6. Control item — 10 ms. Drain the EL7047 status + EL1008 buttons, run
    //    the edge-triggered controller, encode + send the EL7047 control image,
    //    and drive the EL2004 status lamp.
    let healthy_for_control = Arc::clone(&healthy);
    let mut controller = Controller::default();
    // Latest decoded EL7047 status; held across cycles for logging on change.
    let last_logged: Arc<Mutex<Option<el7047::El7047Status>>> = Arc::new(Mutex::new(None));
    let last_logged_for_item = Arc::clone(&last_logged);
    // Fault-lamp blink state, toggled on a sub-cadence of the control loop so
    // the blink is actually visible.
    let mut lamp_blink = false;
    let mut blink_cycles: u32 = 0;
    // Control loop runs at CONTROL_PERIOD (10 ms); toggling every 25 cycles
    // (250 ms) gives a ~2 Hz blink the eye can resolve.
    const BLINK_TOGGLE_CYCLES: u32 = 25;

    exec.add(item_with_triggers(
        |d| -> Result<(), ExecutorError> {
            d.interval(CONTROL_PERIOD);
            Ok(())
        },
        move |_ctx| -> ExecuteResult {
            let healthy_now = healthy_for_control.load(Ordering::Relaxed);

            // Drain the EL7047 status reader, keeping the latest image.
            let mut status = el7047::El7047Status::default();
            let mut got_status = false;
            while let Ok(Some(env)) = reader_el7047.try_recv() {
                if let Some(decoded) = el7047::decode_status(&env.value) {
                    status = decoded;
                    got_status = true;
                }
            }

            // Drain the EL1008 reader, keeping the latest button byte.
            let mut buttons = 0u8;
            let mut dev = generated::EL1008::default();
            while let Ok(Some(env)) = reader_el1008.try_recv() {
                let img: [u8; 1] = env.value;
                // Re-pack the codegen-decoded channels into a plain `u8`
                // bitmask so the pure `control::Controller` stays
                // codegen-agnostic (it sees buttons, not generated types).
                buttons = match dev.decode_inputs(img.view_bits::<Lsb0>()) {
                    Ok(()) => {
                        (u8::from(dev.channel_1.input))
                            | (u8::from(dev.channel_2.input) << 1)
                            | (u8::from(dev.channel_3.input) << 2)
                            | (u8::from(dev.channel_4.input) << 3)
                    }
                    // Decode failure: fall back to the raw byte rather than
                    // panic in the hot loop.
                    Err(_) => img[0],
                };
            }

            // Run the controller and send the EL7047 control image.
            let ctrl = controller.step(buttons, &status, params, healthy_now);
            let _ = writer_el7047.send(&el7047::encode_control(&ctrl));

            // Log EL7047 status transitions (never panic in the loop).
            if got_status {
                let mut last = last_logged_for_item.lock().expect("poisoned");
                if last.as_ref() != Some(&status) {
                    println!(
                        "t=+{:>6}ms  pos={:>8}  ready={} in_target={} busy={} error={} stall={}",
                        started_at.elapsed().as_millis(),
                        status.actual_position,
                        status.ready as u8,
                        status.in_target as u8,
                        status.busy as u8,
                        status.error as u8,
                        status.motor_stall as u8,
                    );
                    *last = Some(status);
                }
            }

            // Status lamp on the EL2004: ch1 = drive ready & connector healthy;
            // ch2 blinks at ~2 Hz while a fault is latched. The blink state
            // flips every BLINK_TOGGLE_CYCLES control cycles (25 * 10 ms =
            // 250 ms half-period) so the blink is visible rather than a 50 Hz
            // dim glow.
            blink_cycles += 1;
            if blink_cycles >= BLINK_TOGGLE_CYCLES {
                blink_cycles = 0;
                lamp_blink = !lamp_blink;
            }
            let mut el2004 = generated::EL2004::default();
            el2004.channel_1.output = status.ready && healthy_now;
            el2004.channel_2.output = status.error && lamp_blink;
            let mut buf = [0u8; 1];
            match el2004.encode_outputs(buf.view_bits_mut::<Lsb0>()) {
                Ok(()) => {
                    let _ = writer_el2004.send(&buf);
                }
                Err(e) => eprintln!("EL2004 encode failed: {e}"),
            }

            Ok(ControlFlow::Continue)
        },
    ))?;

    // 7. Health pump — 250 ms. Subscribe health, log transitions, and publish
    //    the shared `healthy` flag the control loop reads.
    let health_sub = connector.subscribe_health();
    let mut last_state = connector.health().kind();
    healthy.store(
        matches!(last_state, taktora_connector_core::ConnectorHealthKind::Up),
        Ordering::Relaxed,
    );
    eprintln!("ethercat connector health at startup: {last_state:?}");

    let healthy_for_pump = Arc::clone(&healthy);
    exec.add(item_with_triggers(
        |d| -> Result<(), ExecutorError> {
            d.interval(LAMP_PERIOD);
            Ok(())
        },
        move |_ctx| -> ExecuteResult {
            while let Ok(Some(event)) = health_sub.try_next() {
                let new_state = event.to.kind();
                if new_state != last_state {
                    eprintln!(
                        "t=+{:>6}ms  ethercat health: {last_state:?} -> {:?}",
                        started_at.elapsed().as_millis(),
                        event.to
                    );
                    last_state = new_state;
                }
            }
            healthy_for_pump.store(
                matches!(last_state, taktora_connector_core::ConnectorHealthKind::Up),
                Ordering::Relaxed,
            );
            Ok(ControlFlow::Continue)
        },
    ))?;

    // 8. Run until Ctrl-C.
    exec.run()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_defaults_are_sane_move_params() {
        let cli = Cli::parse_from(["ethercat-stepper"]);
        let p = MoveParams {
            step: cli.step,
            velocity: cli.velocity,
            acceleration: cli.accel,
            deceleration: cli.decel,
        };
        assert_eq!(p.step, 3200);
        assert_eq!(p.velocity, 1000);
        assert_eq!(p.acceleration, 1000);
        assert_eq!(p.deceleration, 1000);
        assert!(p.step > 0);
        assert!(p.velocity > 0);
    }
}
