//! Integration example: executor + zenoh connector + JsonCodec over a real
//! `zenoh::Session`. Designed to be run as two terminals:
//!
//!   terminal A: cargo run -- --role pub
//!   terminal B: cargo run -- --role sub
//!
//! Both processes will discover each other as peers on loopback using
//! Zenoh's default peer-to-peer config. No router required.

use core::time::Duration;
use std::sync::Arc;

use clap::{Parser, ValueEnum};
use serde::{Deserialize, Serialize};
use taktora_connector_codec::JsonCodec;
use taktora_connector_core::ChannelDescriptor;
use taktora_connector_host::Connector;
use taktora_connector_zenoh::{
    KeyExprOwned, RealZenohSession, ZenohConnector, ZenohConnectorOptions, ZenohRouting,
    ZenohState,
};
use taktora_executor::{ControlFlow, ExecuteResult, Executor, ExecutorError, item_with_triggers};

const N: usize = 256;
const KEY: &str = "taktora/examples/pubsub-real";

#[derive(Copy, Clone, Debug, ValueEnum)]
enum Role {
    Pub,
    Sub,
    Both,
}

#[derive(Parser, Debug)]
#[command(name = "zenoh-pubsub-real", about = "executor + zenoh (real session) pub/sub example")]
struct Cli {
    /// Process role — `pub`, `sub`, or `both` (loopback peer in a single process).
    #[arg(long, value_enum)]
    role: Role,
    /// Number of ticks to publish before stopping; 0 means run forever.
    #[arg(long, default_value_t = 0)]
    ticks: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Tick {
    seq: u64,
    ts_ms: u64,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // 1. Construct the connector options. Default peer-mode config —
    //    `RealZenohSession` falls back to zenoh's built-in discovery
    //    when no explicit listen/connect locators are supplied.
    let opts = ZenohConnectorOptions::builder()
        .tokio_worker_threads(2)
        .dispatcher_tick(Duration::from_millis(5))
        .build();

    // 2. `RealZenohSession::open` is async, but `main` is sync. Spin up
    //    a small multi-thread runtime just to drive the open future,
    //    then drop it — the session's internal zenoh runtime lives on
    //    independently inside the `Arc<zenoh::Session>` and does not
    //    need our bootstrap runtime to stay alive.
    let session = Arc::new({
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        rt.block_on(RealZenohSession::open(&opts))?
    });

    let state = Arc::new(ZenohState::new(opts));
    let mut connector = ZenohConnector::new(state, Arc::clone(&session), JsonCodec)?;

    // 3. Build matching descriptors. Reader is created before writer so
    //    the subscriber declaration races ahead of the first publish.
    let routing = ZenohRouting::new(KeyExprOwned::try_from(KEY)?);
    let desc =
        ChannelDescriptor::<ZenohRouting, N>::new("taktora.examples.pubsub-real", routing)?;

    let reader = if matches!(cli.role, Role::Sub | Role::Both) {
        Some(connector.create_reader::<Tick, N>(&desc)?)
    } else {
        None
    };
    let writer = if matches!(cli.role, Role::Pub | Role::Both) {
        Some(connector.create_writer::<Tick, N>(&desc)?)
    } else {
        None
    };

    // 4. Build the executor and register the connector.
    let mut exec = Executor::builder().worker_threads(2).build()?;
    connector.register_with(&mut exec)?;

    // 5. Health: subscribe before adding work items so transitions
    //    that occur during the run are observable from the polling item.
    let health_sub = connector.subscribe_health();
    let mut last_state = connector.health().kind();
    eprintln!("zenoh connector health at startup: {last_state:?}");
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
            Ok(ControlFlow::Continue)
        },
    ))?;

    // 6. Publisher item (only if role includes pub): every 500ms,
    //    push a Tick onto the writer.
    if let Some(writer) = writer {
        let total = cli.ticks;
        let mut seq = 0_u64;
        exec.add(item_with_triggers(
            |d| -> Result<(), ExecutorError> {
                d.interval(Duration::from_millis(500));
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
                if total > 0 && seq >= u64::from(total) {
                    // Give the subscriber side one more drain cycle to
                    // round-trip the last publish through zenoh before
                    // tearing the executor down.
                    std::thread::sleep(Duration::from_millis(200));
                    ctx.stop_executor();
                }
                Ok(ControlFlow::Continue)
            },
        ))?;
    }

    // 7. Subscriber item (only if role includes sub): every 100ms,
    //    drain pending Ticks.
    if let Some(reader) = reader {
        exec.add(item_with_triggers(
            |d| -> Result<(), ExecutorError> {
                d.interval(Duration::from_millis(100));
                Ok(())
            },
            move |_| -> ExecuteResult {
                while let Ok(Some(env)) = reader.try_recv() {
                    let t = env.value;
                    println!("recv seq={} ts_ms={}", t.seq, t.ts_ms);
                }
                Ok(ControlFlow::Continue)
            },
        ))?;
    }

    // 8. Run. Exits when the publisher calls `ctx.stop_executor()`
    //    after `--ticks N` publishes, or on Ctrl-C.
    exec.run()?;
    Ok(())
}
