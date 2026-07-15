//! Integration example: executor + zenoh connector + JSON codec.
//!
//! Wires a publisher item (interval-triggered) and a subscriber item
//! (interval-triggered, polls the reader) through `ZenohConnector`
//! backed by `MockZenohSession`, which loops bytes back in-process.
//! Exits after `--ticks N` ticks or on Ctrl-C.

use core::time::Duration;
use std::sync::Arc;

use clap::Parser;
use serde::{Deserialize, Serialize};
use taktora_connector_codec::JsonCodec;
use taktora_connector_core::ChannelDescriptor;
use taktora_connector_host::Connector;
use taktora_connector_zenoh::{
    KeyExprOwned, MockZenohSession, ZenohConnector, ZenohConnectorOptions, ZenohRouting,
    ZenohState,
};
use taktora_executor::{ItemFlow, ExecuteResult, Executor, ExecutorError, item_with_triggers};

const N: usize = 256;
const KEY: &str = "taktora/examples/pubsub";

#[derive(Parser, Debug)]
#[command(name = "zenoh-pubsub-mock", about = "executor + zenoh (mock) pub/sub example")]
struct Cli {
    /// Number of ticks to run before exiting. Each tick is a 200ms publish + drain.
    #[arg(long, default_value_t = 10)]
    ticks: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Tick {
    seq: u64,
    ts_ms: u64,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // 1. Construct the connector backed by MockZenohSession.
    let opts = ZenohConnectorOptions::builder()
        .tokio_worker_threads(1)
        .dispatcher_tick(Duration::from_millis(1))
        .build();
    let state = Arc::new(ZenohState::new(opts));
    let session = Arc::new(MockZenohSession::new());
    let mut connector = ZenohConnector::new(state, Arc::clone(&session), JsonCodec)?;

    // 2. Build matching descriptors. Reader is created BEFORE writer so
    //    the session subscriber is in place before the first publish.
    let routing = ZenohRouting::new(KeyExprOwned::try_from(KEY)?);
    let desc_reader =
        ChannelDescriptor::<ZenohRouting, N>::new("taktora.examples.pubsub", routing.clone())?;
    let desc_writer =
        ChannelDescriptor::<ZenohRouting, N>::new("taktora.examples.pubsub", routing)?;
    let reader = connector.create_reader::<Tick, N>(&desc_reader)?;
    let writer = connector.create_writer::<Tick, N>(&desc_writer)?;

    // 3. Build the executor and register the connector.
    let mut exec = Executor::builder().worker_threads(1).build()?;
    connector.register_with(&mut exec)?;

    // 4. Health: subscribe before adding work items so the publisher/
    //    subscriber observe transitions that occur during the run.
    let health_sub = connector.subscribe_health();
    let mut last_state = connector.health().kind();
    eprintln!("zenoh connector health at startup: {last_state:?}");

    // 5. Publisher item: every 200ms, push a Tick onto the writer.
    let total = cli.ticks;
    let mut seq = 0_u64;
    exec.add(item_with_triggers(
        |d| -> Result<(), ExecutorError> {
            d.interval(Duration::from_millis(200));
            Ok(())
        },
        #[allow(clippy::cast_possible_truncation)]
        move |ctx| -> ExecuteResult {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            let tick = Tick { seq, ts_ms: now_ms };
            writer
                .send(&tick)
                .map_err(|e| -> taktora_executor::ItemError { Box::new(e) })?;
            println!("send seq={seq} ts_ms={now_ms}");
            seq += 1;
            if seq >= u64::from(total) {
                // Give the subscriber one more 50ms drain cycle to round-trip
                // the last publish through MockZenohSession before tearing
                // the executor down.
                std::thread::sleep(Duration::from_millis(150));
                ctx.stop_executor();
                Ok(ItemFlow::StopChain)
            } else {
                Ok(ItemFlow::Continue)
            }
        },
    ))?;

    // 6. Subscriber item: every 50ms, drain pending Ticks.
    exec.add(item_with_triggers(
        |d| -> Result<(), ExecutorError> {
            d.interval(Duration::from_millis(50));
            Ok(())
        },
        move |_| -> ExecuteResult {
            while let Ok(Some(env)) = reader.try_recv() {
                let t = env.value;
                println!("recv seq={} ts_ms={}", t.seq, t.ts_ms);
            }
            Ok(ItemFlow::Continue)
        },
    ))?;

    // 7. Health polling item: every 250ms, drain pending HealthEvents
    //    and log every state transition observed during the run.
    exec.add(item_with_triggers(
        |d| -> Result<(), ExecutorError> {
            d.interval(Duration::from_millis(250));
            Ok(())
        },
        move |_| -> ExecuteResult {
            while let Ok(Some(event)) = health_sub.try_next() {
                let new_state = event.to.kind();
                if new_state != last_state {
                    eprintln!("zenoh connector health: {last_state:?} -> {new_state:?}");
                    last_state = new_state;
                }
            }
            Ok(ItemFlow::Continue)
        },
    ))?;

    // 8. Run. The executor exits when the publisher returns StopChain or on Ctrl-C.
    exec.run()?;
    Ok(())
}
