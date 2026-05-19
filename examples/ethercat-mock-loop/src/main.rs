//! Integration example: executor + ethercat connector (MockBusDriver loopback).
//!
//! A 1 kHz interval item writes a `u16` counter into one PDI routing
//! slice and reads back the same value (lagged by ~1 cycle) from the
//! paired inbound slice. Asserts `lag <= 2` and exits with a
//! `sent=N recv=M max_lag=K` summary after `--ticks N` cycles. Exits
//! non-zero if `max_lag > 2`.
//!
//! Routing layout note: `MockBusDriver`'s `loopback` mode copies a
//! SubDevice's outputs buffer byte-for-byte over its inputs buffer
//! (see `crates/taktora-connector-ethercat/src/mock.rs` in the source
//! repo). Outbound writes therefore appear at the *same* bit offset
//! on the inbound side. Both routings live at bit offset 0 here; the
//! `PdoDirection` (`Rx` vs `Tx`) is what makes one a writer and the
//! other a reader.
//!
//! Encoding note: `JsonCodec` serializes a `u16` as variable-length
//! ASCII (e.g. `5` → `"5"` = 1 byte, `42` → `"42"` = 2 bytes), but
//! `pdi::write_routing` rejects a payload shorter than `bit_length /
//! 8` bytes. To get a fixed-length payload we serialize the counter
//! as a 5-character zero-padded `String` (e.g. `"00042"`), which
//! JSON-encodes to exactly 7 bytes (`"\"00042\""`). The matching
//! routing has `bit_length = 56`.

use core::time::Duration;
use std::sync::{Arc, Mutex};

use clap::Parser;
use serde::{Deserialize, Serialize};
use taktora_connector_codec::JsonCodec;
use taktora_connector_core::ChannelDescriptor;
use taktora_connector_ethercat::{
    EthercatConnector, EthercatConnectorOptions, EthercatRouting, MockBusDriver, PdoDirection,
    connector::EthercatState,
};
use taktora_connector_host::Connector;
use taktora_executor::{ControlFlow, ExecuteResult, Executor, ExecutorError, item_with_triggers};

/// Channel capacity (iceoryx2 service buffer slots).
const N: usize = 256;

/// SubDevice address used by both routings.
const SUBDEV: u16 = 0x0001;

/// PDI buffer size in bytes for both outputs and inputs. Must be
/// at least `(bit_offset + bit_length).div_ceil(8)` = 7 bytes.
const PDI_BYTES: usize = 16;

/// Bit length of the routing slice. Fits the JSON encoding of a
/// 5-character zero-padded `String` (`"\"00042\""` = 7 bytes = 56
/// bits).
const ROUTING_BITS: u16 = 7 * 8;

/// Format a `u16` as a 5-character zero-padded string (`0..=65535`).
/// JSON-encodes to a constant 7 bytes.
fn fmt_counter(v: u16) -> String {
    format!("{v:05}")
}

#[derive(Parser, Debug)]
#[command(
    name = "ethercat-mock-loop",
    about = "executor + ethercat (mock) 1 kHz control loop"
)]
struct Cli {
    /// Number of cycles to run before exiting.
    #[arg(long, default_value_t = 1000)]
    ticks: u32,
}

/// Cumulative loop statistics. Shared between the loop item and the
/// `main` summary print.
#[derive(Default, Serialize, Deserialize)]
struct Stats {
    sent: u64,
    recv: u64,
    max_lag: i64,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // 1. Configure the MockBusDriver. Loopback copies SubDevice
    //    outputs[..] to inputs[..] at the end of every cycle.
    let driver = MockBusDriver::new()
        .with_subdevice_outputs(SUBDEV, vec![0u8; PDI_BYTES])
        .with_subdevice_inputs(SUBDEV, vec![0u8; PDI_BYTES])
        .with_loopback();

    // 2. Build the connector. Cycle time is clamped at 1 ms minimum
    //    by the builder; setting it explicitly documents intent.
    let opts = EthercatConnectorOptions::builder()
        .cycle_time(Duration::from_millis(1))
        .build();
    let state = Arc::new(EthercatState::new(opts));
    let mut connector = EthercatConnector::new(state, driver, JsonCodec::new())?;

    // 3. Paired routings on the same SubDevice, same bit offset, both
    //    sized to the fixed 7-byte JSON payload.
    let routing_rx = EthercatRouting::new(SUBDEV, PdoDirection::Rx, 0, ROUTING_BITS);
    let routing_tx = EthercatRouting::new(SUBDEV, PdoDirection::Tx, 0, ROUTING_BITS);
    let desc_out =
        ChannelDescriptor::<EthercatRouting, N>::new("taktora.examples.ecat.out", routing_rx)?;
    let desc_in =
        ChannelDescriptor::<EthercatRouting, N>::new("taktora.examples.ecat.in", routing_tx)?;
    // Reader created before writer so the gateway-side raw publisher
    // attaches before the first cycle drives inbound bytes onto it.
    let reader = connector.create_reader::<String, N>(&desc_in)?;
    let writer = connector.create_writer::<String, N>(&desc_out)?;

    // 4. Build the executor and register the connector (spawns the
    //    gateway dispatcher loop on its tokio runtime).
    let mut exec = Executor::builder().worker_threads(1).build()?;
    connector.register_with(&mut exec)?;

    // 5. Health subscription before adding items so transitions
    //    during the run are observed by the polling item below.
    let health_sub = connector.subscribe_health();
    let mut last_state = connector.health().kind();
    eprintln!("ethercat connector health at startup: {last_state:?}");

    // 6. Health polling item: every 250 ms, log any state transitions.
    exec.add(item_with_triggers(
        |d| -> Result<(), ExecutorError> {
            d.interval(Duration::from_millis(250));
            Ok(())
        },
        move |_| -> ExecuteResult {
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

    // 7. The control-loop item: 1 kHz, sends a counter and drains the
    //    reader. Stats live in an Arc<Mutex> so the summary print at
    //    the end of `main` can read the final values.
    let stats = Arc::new(Mutex::new(Stats::default()));
    let stats_for_item = Arc::clone(&stats);
    let total = u64::from(cli.ticks);

    exec.add(item_with_triggers(
        |d| -> Result<(), ExecutorError> {
            d.interval(Duration::from_millis(1));
            Ok(())
        },
        move |ctx| -> ExecuteResult {
            let mut s = stats_for_item.lock().expect("stats mutex not poisoned");

            #[allow(clippy::cast_possible_truncation)]
            let seq = s.sent as u16;
            let payload = fmt_counter(seq);
            writer
                .send(&payload)
                .map_err(|e| -> taktora_executor::ItemError { Box::new(e) })?;
            s.sent += 1;

            while let Ok(Some(env)) = reader.try_recv() {
                let recv_value: u16 = env.value.parse().map_err(
                    |e: core::num::ParseIntError| -> taktora_executor::ItemError {
                        Box::new(e)
                    },
                )?;
                // Lag is `sent` (the count of writes issued, including
                // this tick's) minus `recv_value` (the most recent
                // observed counter) minus 1 (for the just-incremented
                // s.sent).
                #[allow(clippy::cast_possible_wrap)]
                let sent_signed = s.sent as i64;
                let lag = sent_signed - i64::from(recv_value) - 1;
                if lag > s.max_lag {
                    s.max_lag = lag;
                }
                s.recv += 1;
            }

            if s.sent >= total {
                drop(s);
                // Give the dispatcher one or two more cycles to drain
                // in-flight outbound bytes before tearing down.
                std::thread::sleep(Duration::from_millis(20));
                ctx.stop_executor();
            }
            Ok(ControlFlow::Continue)
        },
    ))?;

    // 8. Run.
    exec.run()?;

    // 9. Summary line + non-zero exit on broken loopback or lag-bound
    //    breach.
    let s = stats.lock().expect("stats mutex not poisoned");
    println!("sent={} recv={} max_lag={}", s.sent, s.recv, s.max_lag);
    if s.sent > 0 && s.recv == 0 {
        return Err(
            "loopback never round-tripped an envelope (recv == 0); the gateway dispatcher \
             likely failed during the run — check stderr for a panic"
                .into(),
        );
    }
    if s.max_lag > 2 {
        return Err(format!("max_lag={} exceeded threshold 2", s.max_lag).into());
    }
    Ok(())
}
