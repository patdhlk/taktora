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

#[derive(Parser, Debug)]
#[command(
    name = "ethercat-real-bus",
    about = "executor + ethercat connector against a real EK1100 + EL1008"
)]
struct Cli {
    /// Network interface the EK1100 is wired to (e.g. `eth0`).
    #[arg(long, default_value = "eth0")]
    nic: String,

    /// Number of scan cycles (10 ms each) before exiting. `0` runs
    /// forever (until Ctrl-C).
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
    //    behind the `bus-integration` cargo feature. The PDU storage
    //    is a `static` declared above via `declare_pdu_storage!`.
    let driver =
        EthercrabBusDriver::<MAX_SUBDEVICES, MAX_PDI>::new(&EXAMPLE_PDU_STORAGE, opts.clone())?;

    // 3. Connector. RawByteCodec passes raw PDI bytes through unchanged.
    let state = Arc::new(EthercatState::new(opts.clone()));
    let mut connector = EthercatConnector::new(state, driver, RawByteCodec)?;

    // 4. Routing — EL1008 inputs at configured station address
    //    `0x1001`, bit offset 0, 8 bits. PdoDirection::Tx means the
    //    SubDevice writes (Tx) and the master reads.
    let routing = EthercatRouting::new(SUBDEV, PdoDirection::Tx, 0, ROUTING_BITS);
    let desc =
        ChannelDescriptor::<EthercatRouting, N>::new("ethercat.el1008.inputs", routing)?;
    let reader = connector.create_reader::<u8, N>(&desc)?;

    // 5. Executor.
    let mut exec = Executor::builder().worker_threads(1).build()?;
    connector.register_with(&mut exec)?;

    // 6. Health subscription — log Connecting → Up so the user sees
    //    bring-up complete. Then poll for transitions.
    let health_sub = connector.subscribe_health();
    let mut last_state = connector.health().kind();
    eprintln!("ethercat connector health at startup: {last_state:?}");
    exec.add(item_with_triggers(
        |d| -> Result<(), ExecutorError> {
            d.interval(Duration::from_millis(250));
            Ok(())
        },
        move |_ctx| -> ExecuteResult {
            while let Ok(Some(event)) = health_sub.try_next() {
                let new_state = event.to.kind();
                if new_state != last_state {
                    eprintln!("ethercat connector health: {last_state:?} -> {new_state:?}");
                    last_state = new_state;
                }
            }
            Ok(ControlFlow::Continue)
        },
    ))?;

    // 7. Scan-and-print item — 10 ms interval; drain the reader and
    //    print on every change. After `--ticks N` cycles, stop the
    //    executor.
    let started_at = Instant::now();
    let last_value: Arc<Mutex<Option<u8>>> = Arc::new(Mutex::new(None));
    let last_value_for_item = Arc::clone(&last_value);
    let total = cli.ticks;
    let mut cycle = 0_u32;

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
                    println!("t=+{elapsed_ms:>6}ms  bits=0b{v:08b}  decimal={v}");
                    *last = Some(v);
                }
            }
            if total > 0 && cycle >= total {
                ctx.stop_executor();
            }
            Ok(ControlFlow::Continue)
        },
    ))?;

    // 8. Run. Exits when the scan item hits --ticks or on Ctrl-C.
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
