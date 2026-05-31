//! Integration example: executor + ethercat connector against a real
//! WAGO 750-354 EtherCAT coupler carrying a 750-430 (8 digital inputs)
//! and a 750-530 (8 digital outputs), over a real Linux NIC. See
//! README.md for hardware setup and run instructions.
//!
//! Topology — the key contrast with the Beckhoff `ethercat-real-bus`
//! example. The WAGO 750-354 coupler is the ONLY EtherCAT SubDevice on
//! the bus. The 750-430, 750-530, and 750-600 are internal K-bus
//! modules whose I/O is aggregated into the coupler's single process
//! image. `ethercrab`'s `init_single_group` assigns the coupler the
//! configured station address `0x1000`. The 8 input bits (750-430) and
//! 8 output bits (750-530) are separate Tx and Rx slices on that one
//! SubDevice, both at bit offset 0 because they live in distinct
//! process images.
//!
//! Behaviour: each 10 ms scan cycle the example reads the 750-430's 8
//! input bits and writes them straight through to the 750-530's 8
//! output bits (a digital input-to-output mirror), printing on change.

use core::time::Duration;
use std::sync::Arc;
use std::time::Instant;

use clap::Parser;
use taktora_connector_core::{ChannelDescriptor, ConnectorError, PayloadCodec};
use taktora_connector_ethercat::{
    EthercatConnector, EthercatConnectorOptions, EthercatRouting, EthercrabBusDriver, PdoDirection,
    connector::EthercatState, declare_pdu_storage,
};
use taktora_connector_host::Connector;
use taktora_executor::{ControlFlow, ExecuteResult, Executor, ExecutorError, item_with_triggers};

/// Channel capacity (iceoryx2 service buffer slots).
const N: usize = 256;

/// The WAGO 750-354 coupler's EtherCAT configured station address.
/// Unlike the Beckhoff EK1100 (a PDI-less coupler whose terminals are
/// each their own SubDevice), the 750-354 IS the only SubDevice on the
/// bus — the 750-430/750-530/750-600 are internal K-bus modules whose
/// I/O lives in this one SubDevice's process image. `ethercrab`'s
/// `init_single_group` assigns the first SubDevice `0x1000`. If your
/// EtherCAT config tool reports a different address, edit this.
const SUBDEV: u16 = 0x1000;

/// 750-430: 8 digital input bits as one Tx PDI byte at bit offset 0.
const DI_BIT_OFFSET: u32 = 0;
const DI_BITS: u16 = 8;

/// 750-530: 8 digital output bits as one Rx PDI byte at bit offset 0.
/// Inputs and outputs both start at offset 0 because they sit in
/// separate Tx and Rx process images. Adjust if your config tool
/// reports different offsets (e.g. extra modules ahead of these).
const DO_BIT_OFFSET: u32 = 0;
const DO_BITS: u16 = 8;

/// Bus topology bounds passed to `EthercrabBusDriver`. Generous for a
/// Pi + a single WAGO coupler; tune down if memory is tight.
const MAX_SUBDEVICES: usize = 16;
const MAX_PDI: usize = 256;

declare_pdu_storage!(EXAMPLE_PDU_STORAGE);

/// One-byte codec used by this example. `JsonCodec` can't be used here
/// because the WAGO process image is raw bits, not JSON text. This
/// codec round-trips a `u8` to/from a single byte on the wire, matching
/// the 8-bit digital slices on the 750-430 and 750-530.
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
    /// Long-run mode. Logs health at 1/4 Hz; prints pass/fail summary
    /// on exit.
    Endurance,
    /// Drill mode. Runs for `--window` seconds; expects operator to
    /// unplug/replug mid-run; prints pass/fail.
    Drill,
}

#[derive(Parser, Debug)]
#[command(
    name = "ethercat-wago-coupler",
    about = "executor + ethercat connector mirroring a WAGO 750-430 (8 DI) to a 750-530 (8 DO) through a 750-354 coupler"
)]
struct Cli {
    /// Network interface the WAGO 750-354 is wired to (e.g. `eth0`).
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
    /// Number of scan cycles (10 ms each) before exiting. Only used in
    /// `--mode normal`. `0` runs forever.
    #[arg(long, default_value_t = 0)]
    ticks: u32,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // 1. Options.
    let opts = EthercatConnectorOptions::builder()
        .network_interface(&cli.nic)
        .cycle_time(Duration::from_millis(2))
        .build();

    // 2. Driver. EthercrabBusDriver wraps `ethercrab::MainDevice`
    //    behind the `bus-integration` cargo feature. The PDU storage is
    //    the `static` declared above via `declare_pdu_storage!`.
    let driver =
        EthercrabBusDriver::<MAX_SUBDEVICES, MAX_PDI>::new(&EXAMPLE_PDU_STORAGE, opts.clone())?;

    // 3. Connector. RawByteCodec passes raw PDI bytes through unchanged.
    let state = Arc::new(EthercatState::new(opts.clone()));
    let mut connector = EthercatConnector::new(state, driver, RawByteCodec)?;

    // 4. Input routing — 750-430 inputs on the coupler (SUBDEV),
    //    PdoDirection::Tx (SubDevice writes, master reads), bit offset
    //    0, 8 bits.
    let in_routing = EthercatRouting::new(SUBDEV, PdoDirection::Tx, DI_BIT_OFFSET, DI_BITS);
    let in_desc =
        ChannelDescriptor::<EthercatRouting, N>::new("ethercat.wago.750-430.inputs", in_routing)?;
    let reader = connector.create_reader::<u8, N>(&in_desc)?;

    // 4b. Output routing — 750-530 outputs on the SAME SubDevice
    //     (SUBDEV), PdoDirection::Rx (master writes), bit offset 0, 8
    //     bits. This is the single-SubDevice / multi-slice topology:
    //     both routings share SUBDEV and differ only by direction.
    let out_routing = EthercatRouting::new(SUBDEV, PdoDirection::Rx, DO_BIT_OFFSET, DO_BITS);
    let out_desc =
        ChannelDescriptor::<EthercatRouting, N>::new("ethercat.wago.750-530.outputs", out_routing)?;
    let writer = connector.create_writer::<u8, N>(&out_desc)?;

    // 5. Executor.
    let mut exec = Executor::builder().worker_threads(1).build()?;
    connector.register_with(&mut exec)?;

    // 6. Mirror item — 10 ms interval. Drain the reader; on every
    //    change, write the value straight to the 750-530 outputs and
    //    print. In Normal mode, exits after --ticks cycles; in
    //    Endurance/Drill modes, the health-pump item owns the deadline.
    let started_at = Instant::now();
    let total = cli.ticks;
    let mut cycle = 0_u32;
    let mut last_value: Option<u8> = None;
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
                if last_value != Some(v) {
                    let _ = writer.send(&v);
                    let elapsed_ms = started_at.elapsed().as_millis();
                    println!("t=+{elapsed_ms:>6}ms  in=0b{v:08b} -> out=0b{v:08b}  decimal={v}");
                    last_value = Some(v);
                }
            }
            if normal_mode && total > 0 && cycle >= total {
                ctx.stop_executor();
            }
            Ok(ControlFlow::Continue)
        },
    ))?;

    // 7. Mode-specific deadline (None = run forever).
    let deadline: Option<Instant> = match cli.mode {
        Mode::Normal => None,
        Mode::Endurance => Some(started_at + Duration::from_secs(cli.duration)),
        Mode::Drill => Some(started_at + Duration::from_secs(cli.window)),
    };

    // 8. Health pump + drill/endurance tracking. The pump must own
    //    health_sub by move; the per-mode flags are kept locally to the
    //    closure.
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
                    eprintln!(
                        "t=+{:>6}ms  ethercat health: {last_state:?} -> {new_state:?}",
                        started_at.elapsed().as_millis()
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
            Ok(ControlFlow::Continue)
        },
    ))?;

    // 9. Run. Exits when the mirror item hits --ticks, the deadline
    //    fires, or on Ctrl-C.
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
}
