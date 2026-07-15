//! Integration example: executor + ethercat connector against an
//! EK1100 + EL1008 over a real Linux NIC. See README.md for hardware
//! setup and run instructions.
//!
//! Topology assumption: `ethercrab` assigns configured station
//! addresses starting at `0x1000` — EK1100 = `0x1000` (no PDI; it's
//! a bus coupler), EL1008 = `0x1001` with an 8-bit Tx PDO at bit
//! offset 0. If your topology has additional terminals between the
//! EK1100 and EL1008, edit `SUBDEV` to the EL1008's actual
//! configured station address.

use core::time::Duration;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use bitvec::view::BitView;
use clap::Parser;
use taktora_connector_core::{ChannelDescriptor, ConnectorError, PayloadCodec};
use taktora_connector_ethercat::{
    EthercatConnector, EthercatConnectorOptions, EthercatRouting, EthercrabBusDriver, PdoDirection,
    SmWatchdog, SubDeviceMap, connector::EthercatState, declare_pdu_storage,
};
use taktora_connector_host::Connector;
use taktora_ethercat_esi_rt::{EsiDevice, Lsb0};
use taktora_executor::{ItemFlow, ExecuteResult, Executor, ExecutorError, item_with_triggers};

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

/// EL1008's EtherCAT configured station address. `ethercrab`'s
/// `init_single_group` assigns auto-incrementing addresses starting
/// at `0x1000` (EK1100 = 0x1000, EL1008 = 0x1001, EL2004 = 0x1002,
/// etc.). The driver matches on `sd.configured_address()`, not the
/// topology index, so this needs to be the actual EtherCAT station
/// address — not `1`. Adjust if you have additional terminals
/// between the EK1100 and EL1008.
const SUBDEV: u16 = 0x1001;

/// 8 digital input bits, one PDI byte.
const ROUTING_BITS: u16 = 8;

/// EL2004's EtherCAT configured station address. The example assumes
/// the EL2004 sits immediately after the EL1008. Adjust if your
/// topology differs.
const SUBDEV_EL2004: u16 = 0x1002;

/// 4 digital output bits, one PDI byte.
const ROUTING_BITS_EL2004: u16 = 4;

/// Working-counter mapping for WKC-based health (`REQ_0329`). Each
/// SubDevice declares its expected per-cycle WKC contribution: an
/// input-only terminal (TxPDO, master reads) is +2, an output-only
/// terminal (RxPDO, master writes) is +1. EL1008 (input) + EL2004
/// (output) ⇒ expected 3, which matches the bus's observed WKC.
///
/// The PDO-entry lists are empty: EL1008/EL2004 have FIXED PDO
/// mappings, so we do no SDO re-assignment (an empty list is a no-op
/// in the driver) — we only declare the expected WKC. Without this
/// map the connector's expected WKC is 0 and a dead bus would still
/// read as healthy.
static PDO_MAP: &[SubDeviceMap] = &[
    SubDeviceMap::new(SUBDEV, &[], &[], 2),
    // The EL2004 is an OUTPUT slave: per AOU_0016 its SM watchdog must
    // be enabled with a timeout ≤ FTTI/2 (50 ms at the default 100 ms
    // FTTI). The driver programs + read-back-verifies registers
    // 0x0400/0x0420 during bring-up (REQ_0846); on a master stop the
    // EL2004 drops its outputs to safe state within this window —
    // observable in the drill by unplugging mid-run.
    SubDeviceMap::new(SUBDEV_EL2004, &[], &[], 1)
        .with_sm_watchdog(SmWatchdog::from_timeout_us(50_000)),
];

/// Toggle cadence for the output bit during normal and drill modes.
const OUTPUT_TOGGLE_PERIOD: Duration = Duration::from_millis(500);

/// Bus topology bounds passed to `EthercrabBusDriver`. Generous for
/// a Pi + EK1100; tune down if memory is tight.
const MAX_SUBDEVICES: usize = 16;
const MAX_PDI: usize = 256;

declare_pdu_storage!(EXAMPLE_PDU_STORAGE);

/// One-byte codec used by this example. `JsonCodec` can't be used
/// here because the EL1008's PDI is raw bits, not JSON text. This
/// codec round-trips a `u8` to/from a single byte on the wire,
/// matching the EL1008's 8-bit Tx PDO layout.
#[derive(Debug, Clone, Copy, Default)]
struct RawByteCodec;

impl PayloadCodec for RawByteCodec {
    fn format_name(&self) -> &'static str {
        "raw-byte"
    }

    /// # Errors
    ///
    /// Returns [`ConnectorError::Codec`] if `value` does not serialise
    /// to a `u64`-shaped integer in `0..=255`, and
    /// [`ConnectorError::PayloadOverflow`] if `buf` is empty.
    fn encode<T>(&self, value: &T, buf: &mut [u8]) -> Result<usize, ConnectorError>
    where
        T: serde::Serialize,
    {
        let v = serde_json::to_value(value).map_err(|e| ConnectorError::codec("raw-byte", e))?;
        let byte: u8 = v
            .as_u64()
            .ok_or_else(|| {
                ConnectorError::codec(
                    "raw-byte",
                    std::io::Error::other("expected u8-like integer"),
                )
            })?
            .try_into()
            .map_err(|_| {
                ConnectorError::codec(
                    "raw-byte",
                    std::io::Error::other("value does not fit in u8"),
                )
            })?;
        if buf.is_empty() {
            return Err(ConnectorError::PayloadOverflow { actual: 1, max: 0 });
        }
        buf[0] = byte;
        Ok(1)
    }

    /// # Errors
    ///
    /// Returns [`ConnectorError::Codec`] if `buf` is empty or if the
    /// single byte does not deserialise into `T`.
    fn decode<T>(&self, buf: &[u8]) -> Result<T, ConnectorError>
    where
        T: serde::de::DeserializeOwned,
    {
        if buf.is_empty() {
            return Err(ConnectorError::codec(
                "raw-byte",
                std::io::Error::other("empty buffer; expected exactly 1 byte"),
            ));
        }
        let byte = buf[0];
        let v = serde_json::Value::Number(serde_json::Number::from(byte));
        serde_json::from_value(v).map_err(|e| ConnectorError::codec("raw-byte", e))
    }
}

#[derive(clap::ValueEnum, Clone, Debug)]
enum Mode {
    /// Default: 10 ms cycle, runs until --ticks or Ctrl-C.
    Normal,
    /// Long-run mode. Logs WKC + health at 1 Hz; prints
    /// pass/fail summary on exit.
    Endurance,
    /// Drill mode. Runs for `--window` seconds; expects operator
    /// to unplug/replug mid-run; prints pass/fail.
    Drill,
}

#[derive(Parser, Debug)]
#[command(
    name = "ethercat-real-bus",
    about = "executor + ethercat connector against a real EK1100 + EL1008 (+ EL2004)"
)]
struct Cli {
    /// Network interface the EK1100 is wired to (e.g. `eth0`).
    #[arg(long, default_value = "eth0")]
    nic: String,
    /// Mode: normal, endurance, or drill.
    #[arg(long, value_enum, default_value_t = Mode::Normal)]
    mode: Mode,
    /// Endurance duration in seconds. Only used in `--mode endurance`.
    #[arg(long, default_value_t = 3600)]
    duration: u64,
    /// Drill window in seconds. Only used in `--mode drill`.
    #[arg(long, default_value_t = 60)]
    window: u64,
    /// Number of scan cycles (10 ms each) before exiting. Only used
    /// in `--mode normal`. `0` runs forever.
    #[arg(long, default_value_t = 0)]
    ticks: u32,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // 1. Options.
    let opts = EthercatConnectorOptions::builder()
        .network_interface(&cli.nic)
        .cycle_time(Duration::from_millis(2))
        .pdo_map(PDO_MAP)
        .build();

    // 2. Driver. EthercrabBusDriver wraps `ethercrab::MainDevice`
    //    behind the `bus-integration` cargo feature. The PDU storage
    //    is a `static` declared above via `declare_pdu_storage!`.
    let driver =
        EthercrabBusDriver::<MAX_SUBDEVICES, MAX_PDI>::new(&EXAMPLE_PDU_STORAGE, opts.clone())?;

    // 3. Connector. RawByteCodec passes raw PDI bytes through unchanged.
    let state = Arc::new(EthercatState::new(opts.clone()));
    let mut connector = EthercatConnector::new(state, driver, RawByteCodec)?;

    // Cross-check the hand-written routing bit-widths against the
    // ESI-generated drivers' byte-rounded process-image sizes. EL1008 = 8
    // input bits = 1 byte; EL2004 = 4 output bits = 1 byte. If a future ESI
    // edit changes a device's PDI size, this trips at startup instead of
    // silently mis-routing PDOs.
    debug_assert_eq!(
        generated::EL1008::default().input_len(),
        usize::from(ROUTING_BITS).div_ceil(8),
        "EL1008 input_len disagrees with ROUTING_BITS",
    );
    debug_assert_eq!(
        generated::EL2004::default().output_len(),
        usize::from(ROUTING_BITS_EL2004).div_ceil(8),
        "EL2004 output_len disagrees with ROUTING_BITS_EL2004",
    );

    // 4. Routing — EL1008 inputs at configured station address
    //    `0x1001`, bit offset 0, 8 bits. PdoDirection::Tx means the
    //    SubDevice writes (Tx) and the master reads.
    let routing = EthercatRouting::new(SUBDEV, PdoDirection::Tx, 0, ROUTING_BITS);
    let desc = ChannelDescriptor::<EthercatRouting, N>::new("ethercat.el1008.inputs", routing)?;
    let reader = connector.create_reader::<u8, N>(&desc)?;

    // 4b. EL2004 outputs routing: configured station address 0x1002,
    //     PdoDirection::Rx (master writes), bit offset 0, 4 bits.
    let routing_el2004 =
        EthercatRouting::new(SUBDEV_EL2004, PdoDirection::Rx, 0, ROUTING_BITS_EL2004);
    let desc_el2004 =
        ChannelDescriptor::<EthercatRouting, N>::new("ethercat.el2004.outputs", routing_el2004)?;
    let writer_el2004 = connector.create_writer::<u8, N>(&desc_el2004)?;

    // 5. Executor.
    let mut exec = Executor::builder().worker_threads(1).build()?;
    connector.register_with(&mut exec)?;

    // 6. Scan-and-print item — 10 ms interval; drain the reader and
    //    print on every change. In Normal mode, exits after --ticks
    //    cycles; in Endurance/Drill modes, the health-pump item below
    //    owns the deadline.
    let started_at = Instant::now();
    let last_value: Arc<Mutex<Option<u8>>> = Arc::new(Mutex::new(None));
    let last_value_for_item = Arc::clone(&last_value);
    let total = cli.ticks;
    let mut cycle = 0_u32;
    let normal_mode = matches!(cli.mode, Mode::Normal);

    exec.add(item_with_triggers(
        |d| -> Result<(), ExecutorError> {
            d.interval(Duration::from_millis(10));
            Ok(())
        },
        move |ctx| -> ExecuteResult {
            cycle = cycle.saturating_add(1);
            while let Ok(Some(env)) = reader.try_recv() {
                let v: u8 = env.value;
                let mut last = last_value_for_item.lock().expect("poisoned");
                if last.as_ref().copied() != Some(v) {
                    let elapsed_ms = started_at.elapsed().as_millis();
                    // Typed layer: decode the raw PDI byte into the
                    // ESI-generated EL1008 driver and print the NAMED channels
                    // alongside the raw bits. The driver's bit layout comes
                    // straight from `esi/beckhoff_el1008.xml`. A decode error
                    // is logged and skipped — never panic in the hot loop.
                    let mut dev = generated::EL1008::default();
                    match dev.decode_inputs([v].view_bits::<Lsb0>()) {
                        Ok(()) => {
                            // The generated EL1008 carries its named channels
                            // under the active op-mode; the EL1008 has a single
                            // `Default` mode, so this binding is irrefutable.
                            let generated::EL1008OpMode::Default(m) = &dev.mode;
                            println!(
                                "t=+{elapsed_ms:>6}ms  bits=0b{v:08b}  decimal={v}  \
                                 ch1={} ch2={} ch3={} ch4={} ch5={} ch6={} ch7={} ch8={}",
                                m.inputs.channel_1.input as u8,
                                m.inputs.channel_2.input as u8,
                                m.inputs.channel_3.input as u8,
                                m.inputs.channel_4.input as u8,
                                m.inputs.channel_5.input as u8,
                                m.inputs.channel_6.input as u8,
                                m.inputs.channel_7.input as u8,
                                m.inputs.channel_8.input as u8,
                            );
                        }
                        Err(e) => {
                            eprintln!(
                                "t=+{elapsed_ms:>6}ms  EL1008 decode failed for \
                                 bits=0b{v:08b}: {e}"
                            );
                        }
                    }
                    *last = Some(v);
                }
            }
            if normal_mode && total > 0 && cycle >= total {
                ctx.stop_executor();
            }
            Ok(ItemFlow::Continue)
        },
    ))?;

    // 7. Toggle EL2004 output bit 0 on a 500 ms cadence. Demonstrates
    //    with_subdevice_outputs_mut end-to-end against the real bus.
    let mut state_bool = false;
    exec.add(item_with_triggers(
        |d| -> Result<(), ExecutorError> {
            d.interval(OUTPUT_TOGGLE_PERIOD);
            Ok(())
        },
        move |_ctx| -> ExecuteResult {
            state_bool = !state_bool;
            // Typed layer: drive channel 1 of the ESI-generated EL2004 and let
            // it encode the output PDI byte. Channels 2..4 stay false. The bit
            // layout comes straight from `esi/beckhoff_el2004.xml`.
            let mut el2004 = generated::EL2004::default();
            let generated::EL2004OpMode::Default(m) = &mut el2004.mode;
            m.outputs.channel_1.output = state_bool;
            let mut buf = [0u8; 1];
            match el2004.encode_outputs(buf.view_bits_mut::<Lsb0>()) {
                Ok(()) => {
                    let _ = writer_el2004.send(&buf[0]);
                }
                Err(e) => eprintln!("EL2004 encode failed: {e}"),
            }
            Ok(ItemFlow::Continue)
        },
    ))?;

    // 8. Mode-specific deadline (None = run forever).
    let deadline: Option<Instant> = match cli.mode {
        Mode::Normal => None,
        Mode::Endurance => Some(started_at + Duration::from_secs(cli.duration)),
        Mode::Drill => Some(started_at + Duration::from_secs(cli.window)),
    };

    // 9. Health pump + drill/endurance tracking. The pump must own
    //    health_sub by move; the per-mode flags are kept locally to
    //    the closure.
    let health_sub = connector.subscribe_health();
    let mut last_state = connector.health().kind();
    eprintln!("ethercat connector health at startup: {last_state:?}");

    let mode_for_pump = cli.mode.clone();
    let mut drill_seen_degraded = false;
    let mut drill_seen_recover_up = false;
    let mut endurance_terminal_down = false;

    exec.add(item_with_triggers(
        |d| -> Result<(), ExecutorError> {
            d.interval(Duration::from_millis(250));
            Ok(())
        },
        move |ctx| -> ExecuteResult {
            while let Ok(Some(event)) = health_sub.try_next() {
                let new_state = event.to.kind();
                if new_state != last_state {
                    // Print the full health value, not just the kind, so a
                    // `Degraded { reason: … }` surfaces WHY (WKC mismatch vs
                    // dropped inbound frames) instead of a bare `Degraded`.
                    eprintln!(
                        "t=+{:>6}ms  ethercat health: {last_state:?} -> {:?}",
                        started_at.elapsed().as_millis(),
                        event.to
                    );
                    use taktora_connector_core::ConnectorHealthKind::*;
                    match new_state {
                        Degraded => drill_seen_degraded = true,
                        Up if drill_seen_degraded => drill_seen_recover_up = true,
                        Down => endurance_terminal_down = true,
                        _ => {}
                    }
                    last_state = new_state;
                }
            }
            // Mode deadline.
            if let Some(d) = deadline
                && Instant::now() >= d
            {
                match mode_for_pump {
                    Mode::Endurance => {
                        eprintln!("endurance summary: terminal_down={endurance_terminal_down}");
                    }
                    Mode::Drill => eprintln!(
                        "drill summary: saw_degraded={drill_seen_degraded} \
                         saw_recover_up={drill_seen_recover_up}"
                    ),
                    Mode::Normal => {}
                }
                ctx.stop_executor();
            }
            Ok(ItemFlow::Continue)
        },
    ))?;

    // 10. Run. Exits when the scan item hits --ticks, the deadline
    //     fires, or on Ctrl-C.
    exec.run()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_zero() {
        let mut buf = [0_u8; 1];
        let n = RawByteCodec.encode(&0_u8, &mut buf).unwrap();
        assert_eq!(n, 1);
        assert_eq!(buf[0], 0);
        let v: u8 = RawByteCodec.decode(&buf[..n]).unwrap();
        assert_eq!(v, 0);
    }

    #[test]
    fn round_trips_max_byte() {
        let mut buf = [0_u8; 1];
        let n = RawByteCodec.encode(&255_u8, &mut buf).unwrap();
        assert_eq!(n, 1);
        assert_eq!(buf[0], 255);
        let v: u8 = RawByteCodec.decode(&buf[..n]).unwrap();
        assert_eq!(v, 255);
    }

    #[test]
    fn decode_empty_buf_errors() {
        let result: Result<u8, _> = RawByteCodec.decode(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn encode_empty_buf_errors() {
        let mut buf: [u8; 0] = [];
        let result = RawByteCodec.encode(&42_u8, &mut buf);
        assert!(result.is_err());
    }

    /// Exercise ONLY the ESI-generated typed layer: decode a known EL1008 PDI
    /// byte into named channels, then set an EL2004 channel and encode it back
    /// to a byte. This runs without the bus (no `bus-integration`/ethercrab),
    /// proving the codegen spine compiles and round-trips on any host.
    #[test]
    fn esi_typed_layer_round_trips() {
        // EL1008: 0b1010_1010 in Lsb0 -> ch1=0 ch2=1 ch3=0 ch4=1 ...
        let mut el1008 = generated::EL1008::default();
        el1008
            .decode_inputs([0b1010_1010u8].view_bits::<Lsb0>())
            .expect("EL1008 decode should succeed");
        let generated::EL1008OpMode::Default(m) = &el1008.mode;
        assert!(!m.inputs.channel_1.input);
        assert!(m.inputs.channel_2.input);
        assert!(!m.inputs.channel_7.input);
        assert!(m.inputs.channel_8.input);
        assert_eq!(el1008.input_len(), 1);
        assert_eq!(el1008.output_len(), 0);

        // EL2004: set channel 1 only -> bit0 high -> 0b0000_0001.
        let mut el2004 = generated::EL2004::default();
        let generated::EL2004OpMode::Default(m) = &mut el2004.mode;
        m.outputs.channel_1.output = true;
        let mut buf = [0u8; 1];
        el2004
            .encode_outputs(buf.view_bits_mut::<Lsb0>())
            .expect("EL2004 encode should succeed");
        assert_eq!(buf[0], 0b0000_0001);
        assert_eq!(el2004.output_len(), 1);
        assert_eq!(el2004.input_len(), 0);
    }
}
