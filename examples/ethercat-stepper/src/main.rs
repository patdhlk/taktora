//! Integration example: executor + ethercat connector driving a Beckhoff
//! EL7047 stepper terminal off EL1008 button presses, with the EL2004 as a
//! drive-status lamp. See README.md for hardware setup and run instructions.
//!
//! Motion uses the Beckhoff **positioning interface** (NOT CiA 402): each
//! index press fires a relative move whose trapezoid the terminal runs itself.
//! Enable = STM Control bit0; move = Target/Velocity/Start-type in the POS
//! Control PDO plus an Execute edge. All three terminals are driven through
//! the ESI-generated typed device drivers: the EL7047 runs in its generated
//! `PositioningInterface` mode, mapped to/from the domain control/status
//! types by `el7047_adapter`; EL1008/EL2004 ride their generated `Default`
//! modes.
//!
//! Topology and PDO assignment are declared in `network.yaml` and compiled to
//! `generated_net` by the build script. Device order in that file is bus order
//! (`ethercrab` assigns 0x1000 + n): EK1100 = 0x1000, EL1008 = 0x1001,
//! EL2004 = 0x1002, EL7047 = 0x1003. Reorder the `devices:` list if your bus
//! differs.

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
    EthercatConnector, EthercatConnectorOptions, EthercrabBusDriver, connector::EthercatState,
    declare_pdu_storage,
};
use taktora_connector_host::Connector;
use taktora_ethercat_esi_rt::{EsiDevice, Lsb0};
use taktora_executor::{ControlFlow, ExecuteResult, Executor, ExecutorError, item_with_triggers};

use ethercat_stepper::codec::RawImageCodec;
use ethercat_stepper::control::{Controller, MoveParams};
use ethercat_stepper::{el7047_adapter, el7047_domain, generated, generated_net};

/// Channel capacity (iceoryx2 service buffer slots).
const N: usize = 256;

/// Control-loop cadence.
const CONTROL_PERIOD: Duration = Duration::from_millis(10);
/// Lamp/health cadence.
const LAMP_PERIOD: Duration = Duration::from_millis(250);

/// Bus topology bounds passed to `EthercrabBusDriver`; keep in sync with
/// `bus.max_subdevices` / `bus.max_pdi_bytes` in `network.yaml` (the codegen
/// does not emit these as constants).
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

    // 0. Generated EL7047 device, pinned to the positioning-interface mode and
    //    reused across cycles (input images decode into it, output images encode
    //    out of it; the domain control/status surface is mapped via
    //    `el7047_adapter`). Used only for the byte-level codec — the bus PDO
    //    assignment now comes from generated_net::PDO_MAP (network.yaml).
    let mut el7047_dev = generated::EL7047 {
        mode: generated::EL7047OpMode::PositioningInterface(Default::default()),
    };

    // 1. Options. PDO map, watchdogs, WKC, and startup SDOs are all generated
    //    from network.yaml. Identity bring-up expectations are exposed as
    //    generated_net::EXPECTED_IDENTITIES (logged below; runtime enforcement
    //    is a connector TODO).
    let opts = EthercatConnectorOptions::builder()
        .network_interface(&cli.nic)
        .cycle_time(Duration::from_millis(2))
        .pdo_map(generated_net::PDO_MAP)
        .build();

    // Informational only: the connector has no identity-check API yet, so this
    // logs (does not enforce) the expected per-device identities — a hardware
    // mismatch is then visible in the terminal at startup.
    for id in generated_net::EXPECTED_IDENTITIES {
        eprintln!(
            "expecting device @ {:#06x}: vendor={:#x} product={:#x} rev={:#x}",
            id.address, id.vendor_id, id.product_code, id.revision
        );
    }

    // 2. Driver.
    let driver =
        EthercrabBusDriver::<MAX_SUBDEVICES, MAX_PDI>::new(&EXAMPLE_PDU_STORAGE, opts.clone())?;

    // 3. Connector. ONE codec for every channel: RawImageCodec round-trips a
    //    fixed-size `[u8; LEN]` image verbatim.
    let state = Arc::new(EthercatState::new(opts.clone()));
    let mut connector = EthercatConnector::new(state, driver, RawImageCodec)?;

    // 4. Routing + channels, from the generated routing consts.
    let el1008_desc = ChannelDescriptor::<_, N>::new(
        generated_net::ETHERCAT_EL1008_INPUTS_NAME,
        generated_net::ETHERCAT_EL1008_INPUTS,
    )?;
    let reader_el1008 = connector.create_reader::<[u8; 1], N>(&el1008_desc)?;

    let el2004_desc = ChannelDescriptor::<_, N>::new(
        generated_net::ETHERCAT_EL2004_OUTPUTS_NAME,
        generated_net::ETHERCAT_EL2004_OUTPUTS,
    )?;
    let writer_el2004 = connector.create_writer::<[u8; 1], N>(&el2004_desc)?;

    let el7047_out_desc = ChannelDescriptor::<_, N>::new(
        generated_net::ETHERCAT_EL7047_CONTROL_NAME,
        generated_net::ETHERCAT_EL7047_CONTROL,
    )?;
    let writer_el7047 =
        connector.create_writer::<[u8; el7047_domain::OUTPUT_LEN], N>(&el7047_out_desc)?;

    let el7047_in_desc = ChannelDescriptor::<_, N>::new(
        generated_net::ETHERCAT_EL7047_STATUS_NAME,
        generated_net::ETHERCAT_EL7047_STATUS,
    )?;
    let reader_el7047 =
        connector.create_reader::<[u8; el7047_domain::INPUT_LEN], N>(&el7047_in_desc)?;

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
    let last_logged: Arc<Mutex<Option<el7047_domain::El7047Status>>> = Arc::new(Mutex::new(None));
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

            // Drain the EL7047 status reader, keeping the latest image. Decode
            // it through the generated positioning-interface device, then map to
            // the domain status via the adapter so the controller stays
            // codegen-agnostic.
            let mut status = el7047_domain::El7047Status::default();
            let mut got_status = false;
            while let Ok(Some(env)) = reader_el7047.try_recv() {
                let img: [u8; el7047_domain::INPUT_LEN] = env.value;
                if el7047_dev.decode_inputs(img.view_bits::<Lsb0>()).is_ok() {
                    status = el7047_adapter::read_status(&el7047_dev);
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
                        // EL1008OpMode has a single `Default` variant.
                        let generated::EL1008OpMode::Default(m) = &dev.mode;
                        (u8::from(m.inputs.channel_1.input))
                            | (u8::from(m.inputs.channel_2.input) << 1)
                            | (u8::from(m.inputs.channel_3.input) << 2)
                            | (u8::from(m.inputs.channel_4.input) << 3)
                            | (u8::from(m.inputs.channel_5.input) << 4)
                            | (u8::from(m.inputs.channel_6.input) << 5)
                            | (u8::from(m.inputs.channel_7.input) << 6)
                            | (u8::from(m.inputs.channel_8.input) << 7)
                    }
                    // Decode failure: fall back to the raw byte rather than
                    // panic in the hot loop.
                    Err(_) => img[0],
                };
            }

            // Run the controller, map the domain control into the generated
            // device, encode the output image, and send it.
            let ctrl = controller.step(buttons, &status, params, healthy_now);
            el7047_adapter::apply_control(&mut el7047_dev, &ctrl);
            let mut out = [0u8; el7047_domain::OUTPUT_LEN];
            match el7047_dev.encode_outputs(out.view_bits_mut::<Lsb0>()) {
                Ok(()) => {
                    let _ = writer_el7047.send(&out);
                }
                Err(e) => eprintln!("EL7047 encode failed: {e}"),
            }

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
            // EL2004OpMode has a single `Default` variant.
            let generated::EL2004OpMode::Default(m) = &mut el2004.mode;
            m.outputs.channel_1.output = status.ready && healthy_now;
            m.outputs.channel_2.output = status.error && lamp_blink;
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
