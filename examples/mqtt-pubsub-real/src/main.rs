//! Integration example: executor + mqtt connector + JSON codec over a
//! **real** broker.
//!
//! Wires a publisher item (interval-triggered) and a subscriber item
//! (interval-triggered, polls the reader) through `MqttConnector` backed by
//! `RealMqttSession` — the `rumqttc-integration` backend talking to an
//! actual MQTT broker. Because the round-trip goes out to the broker and
//! back, there is NO `deliver_inbound`: a real broker echoes a published
//! message to every matching subscription, so the loopback is genuine.
//!
//! Point it at a broker with `--broker`/`--port` (or `MQTT_BROKER`/
//! `MQTT_PORT`). The bundled `docker-compose.yml` stands up a mosquitto
//! broker and runs this binary against it:
//!
//!   docker compose up --build --abort-on-container-exit
//!
//! Exits after `--ticks N` publishes or on Ctrl-C.

use core::time::Duration;
use std::sync::Arc;

use clap::Parser;
use serde::{Deserialize, Serialize};
use taktora_connector_codec::JsonCodec;
use taktora_connector_core::ChannelDescriptor;
use taktora_connector_host::Connector;
use taktora_connector_mqtt::{
    MqttConnector, MqttConnectorOptions, MqttQos, MqttRouting, MqttState, MqttTopic,
    RealMqttSession,
};
use taktora_executor::{ControlFlow, ExecuteResult, Executor, ExecutorError, item_with_triggers};

const N: usize = 256;
const TOPIC: &str = "taktora/examples/pubsub-real";

#[derive(Parser, Debug)]
#[command(
    name = "mqtt-pubsub-real",
    about = "executor + mqtt (real broker) pub/sub example"
)]
struct Cli {
    /// Broker host. Falls back to `$MQTT_BROKER`, then `127.0.0.1`.
    #[arg(long)]
    broker: Option<String>,
    /// Broker port. Falls back to `$MQTT_PORT`, then `1883`.
    #[arg(long)]
    port: Option<u16>,
    /// Number of ticks to run before exiting. Each tick is a 200ms publish.
    #[arg(long, default_value_t = 20)]
    ticks: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Tick {
    seq: u64,
    ts_ms: u64,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Resolve broker host/port: CLI arg wins, then env, then the default.
    let broker = cli
        .broker
        .or_else(|| std::env::var("MQTT_BROKER").ok())
        .unwrap_or_else(|| "127.0.0.1".to_owned());
    let port = cli
        .port
        .or_else(|| std::env::var("MQTT_PORT").ok().and_then(|p| p.parse().ok()))
        .unwrap_or(1883);

    // 1. Connector options carry the broker address the real session dials.
    let opts = MqttConnectorOptions::builder()
        .broker_host(broker.clone())
        .broker_port(port)
        .client_id("mqtt-pubsub-real")
        .build();

    // 2. `RealMqttSession::connect` spawns its event-loop *pump* task with
    //    `tokio::spawn`, so it must run inside a tokio runtime context — and
    //    that pump keeps living on the runtime it was spawned on.
    //    `Executor::run()` below is sync and blocks the main thread, so the
    //    pump must live on a runtime whose worker threads stay alive for the
    //    whole run. We therefore build a multi-thread runtime, `block_on` the
    //    connect to obtain the session, and — crucially — keep `rt` in scope
    //    (do NOT drop it) so its workers keep polling the pump while
    //    `exec.run()` blocks. (Contrast `zenoh-pubsub-real`, whose session
    //    owns an independent internal runtime and so can drop the bootstrap
    //    runtime after `open`.)
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let session = Arc::new(rt.block_on(async { RealMqttSession::connect(&opts) })?);
    eprintln!("mqtt connecting to {broker}:{port} (topic {TOPIC})");

    let state = Arc::new(MqttState::new(opts));
    let mut connector = MqttConnector::new(state, Arc::clone(&session), JsonCodec)?;

    // 3. Build matching descriptors. Reader is created BEFORE writer so the
    //    broker SUBSCRIBE is registered before the first publish — otherwise
    //    the broker would drop early publishes for a topic no one is yet
    //    subscribed to.
    let routing = MqttRouting::new(MqttTopic::new(TOPIC)?, MqttQos::AtLeastOnce);
    let desc_reader =
        ChannelDescriptor::<MqttRouting, N>::new("taktora.examples.pubsub-real", routing.clone())?;
    let desc_writer =
        ChannelDescriptor::<MqttRouting, N>::new("taktora.examples.pubsub-real", routing)?;
    let reader = connector.create_reader::<Tick, N>(&desc_reader)?;
    let writer = connector.create_writer::<Tick, N>(&desc_writer)?;

    // 4. Build the executor and register the connector.
    let mut exec = Executor::builder().worker_threads(1).build()?;
    connector.register_with(&mut exec)?;

    // 5. Health: subscribe before adding work items so the publisher/
    //    subscriber observe the Connecting -> Connected transition the pump
    //    drives once the broker's CONNACK arrives.
    let health_sub = connector.subscribe_health();
    let mut last_state = connector.health().kind();
    eprintln!("mqtt connector health at startup: {last_state:?}");

    // 6. Publisher item: every 200ms, publish a Tick through the connector.
    //    No `deliver_inbound` here — the real broker echoes the publish back
    //    to our subscription, so the subscriber item below sees it for real.
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
                // Give the subscriber a few more drain cycles to round-trip
                // the last publishes through the broker before teardown.
                std::thread::sleep(Duration::from_millis(300));
                ctx.stop_executor();
                Ok(ControlFlow::StopChain)
            } else {
                Ok(ControlFlow::Continue)
            }
        },
    ))?;

    // 7. Subscriber item: every 50ms, drain pending Ticks the broker echoed.
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
            Ok(ControlFlow::Continue)
        },
    ))?;

    // 8. Health polling item: every 250ms, log every state transition.
    exec.add(item_with_triggers(
        |d| -> Result<(), ExecutorError> {
            d.interval(Duration::from_millis(250));
            Ok(())
        },
        move |_| -> ExecuteResult {
            while let Ok(Some(event)) = health_sub.try_next() {
                let new_state = event.to.kind();
                if new_state != last_state {
                    eprintln!("mqtt connector health: {last_state:?} -> {new_state:?}");
                    last_state = new_state;
                }
            }
            Ok(ControlFlow::Continue)
        },
    ))?;

    // 9. Run. Blocks the main thread while the tokio `rt` workers keep the
    //    event-loop pump alive. Exits when the publisher returns StopChain
    //    after `--ticks N` publishes, or on Ctrl-C. `rt` is dropped only
    //    after `run()` returns, tearing the pump down last.
    exec.run()?;
    Ok(())
}
