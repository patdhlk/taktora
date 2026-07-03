//! Integration example: executor + mqtt connector + JSON codec.
//!
//! Wires a publisher item (interval-triggered) and a subscriber item
//! (interval-triggered, polls the reader) through `MqttConnector`
//! backed by `MockMqttSession`, which loops bytes back in-process — no
//! broker, no network, no `rumqttc-integration` feature flag. Exits
//! after `--ticks N` ticks or on Ctrl-C.

use core::time::Duration;
use std::sync::Arc;

use clap::Parser;
use serde::{Deserialize, Serialize};
use taktora_connector_codec::JsonCodec;
use taktora_connector_core::{ChannelDescriptor, PayloadCodec};
use taktora_connector_host::Connector;
use taktora_connector_mqtt::{
    MockMqttSession, MqttConnector, MqttConnectorOptions, MqttQos, MqttRouting, MqttState, MqttTopic,
};
use taktora_executor::{ControlFlow, ExecuteResult, Executor, ExecutorError, item_with_triggers};

const N: usize = 256;
const TOPIC: &str = "taktora/examples/pubsub";

#[derive(Parser, Debug)]
#[command(name = "mqtt-pubsub-mock", about = "executor + mqtt (mock) pub/sub example")]
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

    // 1. Construct the connector backed by MockMqttSession. The mock needs
    //    no broker address — it loops published bytes back to matching
    //    subscriptions in-process.
    let opts = MqttConnectorOptions::builder()
        .client_id("mqtt-pubsub-mock")
        .build();
    let state = Arc::new(MqttState::new(opts));
    let session = Arc::new(MockMqttSession::new());
    let mut connector = MqttConnector::new(state, Arc::clone(&session), JsonCodec)?;

    // 2. Build matching descriptors. Reader is created BEFORE writer so the
    //    session subscription is in place before the first publish. The
    //    reader's routing derives an exact-topic filter that matches the
    //    writer's publish topic, so the loopback delivers.
    let routing = MqttRouting::new(MqttTopic::new(TOPIC)?, MqttQos::AtLeastOnce);
    let desc_reader =
        ChannelDescriptor::<MqttRouting, N>::new("taktora.examples.pubsub", routing.clone())?;
    let desc_writer =
        ChannelDescriptor::<MqttRouting, N>::new("taktora.examples.pubsub", routing)?;
    let reader = connector.create_reader::<Tick, N>(&desc_reader)?;
    let writer = connector.create_writer::<Tick, N>(&desc_writer)?;

    // 3. Build the executor and register the connector.
    let mut exec = Executor::builder().worker_threads(1).build()?;
    connector.register_with(&mut exec)?;

    // 4. Health: subscribe before adding work items so the publisher/
    //    subscriber observe transitions that occur during the run.
    let health_sub = connector.subscribe_health();
    let mut last_state = connector.health().kind();
    eprintln!("mqtt connector health at startup: {last_state:?}");

    // 5. Publisher item: every 200ms, publish a Tick through the connector
    //    and simulate the broker echoing it back to our subscription.
    //
    //    Unlike a self-looping mock, `MockMqttSession` records outbound
    //    publishes but does not feed them back inbound — inbound delivery is
    //    an explicit seam (`deliver_inbound`). So we mirror what a real broker
    //    would do for a topic we are also subscribed to: after the outbound
    //    `writer.send`, hand the same bytes to `deliver_inbound`, which the
    //    connector fans out to every matching reader.
    let total = cli.ticks;
    let mut seq = 0_u64;
    let echo_session = Arc::clone(&session);
    let echo_topic = MqttTopic::new(TOPIC)?;
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
            // Simulate the broker delivering the publish back to subscribers.
            let mut buf = [0u8; N];
            if let Ok(len) = JsonCodec.encode(&tick, &mut buf) {
                echo_session.deliver_inbound(&echo_topic, &buf[..len]);
            }
            println!("send seq={seq} ts_ms={now_ms}");
            seq += 1;
            if seq >= u64::from(total) {
                // Give the subscriber one more drain cycle to round-trip the
                // last publish through MockMqttSession before teardown.
                std::thread::sleep(Duration::from_millis(150));
                ctx.stop_executor();
                Ok(ControlFlow::StopChain)
            } else {
                Ok(ControlFlow::Continue)
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
            Ok(ControlFlow::Continue)
        },
    ))?;

    // 7. Health polling item: every 250ms, log every state transition.
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

    // 8. Run. The executor exits when the publisher returns StopChain or on Ctrl-C.
    exec.run()?;
    Ok(())
}
