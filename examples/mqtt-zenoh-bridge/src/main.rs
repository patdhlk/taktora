//! Golden-path example: a protocol bridge.
//!
//! MQTT ingress  ->  executor transform  ->  Zenoh egress
//!
//! A synthetic sensor "publishes" temperature readings onto an MQTT topic
//! (backed by `MockMqttSession`, so no broker). One executor item drains
//! those readings, applies a REAL transform — Celsius→Fahrenheit plus a
//! threshold alarm decision — and re-publishes the decision onto a Zenoh
//! key expression (backed by `MockZenohSession`, so no router). A sink
//! item drains the Zenoh side and prints what came out.
//!
//! Why this is the golden path: the whole connector framework's value
//! prop is visible in one file. TWO unrelated wire protocols (MQTT and
//! Zenoh) are wired to the SAME cyclic `Executor` and driven through the
//! SAME `ChannelReader` / `ChannelWriter` seam. The bridge item below
//! never mentions MQTT or Zenoh — it calls `reader.try_recv()` and
//! `writer.send(...)` exactly as it would against any other connector.
//! Swap a `MockMqttSession` for a `RealMqttSession`, or Zenoh for
//! EtherCAT, and the transform code does not change. No broker, no
//! router, no hardware needed to run it.
//!
//! Run: `cargo run -- --ticks 5`

use core::time::Duration;
use std::sync::{Arc, Mutex};

use clap::Parser;
use serde::{Deserialize, Serialize};
use taktora_connector_codec::JsonCodec;
use taktora_connector_core::{ChannelDescriptor, PayloadCodec};
use taktora_connector_host::Connector;
use taktora_connector_mqtt::{
    MockMqttSession, MqttConnector, MqttConnectorOptions, MqttQos, MqttRouting, MqttState,
    MqttTopic,
};
use taktora_connector_zenoh::{
    KeyExprOwned, MockZenohSession, ZenohConnector, ZenohConnectorOptions, ZenohRouting, ZenohState,
};
use taktora_executor::{ControlFlow, ExecuteResult, Executor, ExecutorError, item_with_triggers};

/// Channel capacity (iceoryx2 service buffer slots) and codec scratch size.
const N: usize = 256;

/// MQTT topic the synthetic sensor publishes raw Celsius readings to.
const MQTT_TOPIC: &str = "taktora/examples/sensors/temperature";

/// Zenoh key expression the bridge re-publishes alarm decisions to.
const ZENOH_KEY: &str = "taktora/examples/bridge/alarms";

/// Warn at or above this temperature; critical at [`CRITICAL_C`].
const WARN_C: f64 = 60.0;
const CRITICAL_C: f64 = 80.0;

#[derive(Parser, Debug)]
#[command(
    name = "mqtt-zenoh-bridge",
    about = "executor + mqtt (mock) ingress -> transform -> zenoh (mock) egress"
)]
struct Cli {
    /// Number of ingress readings to inject before exiting. Each is a
    /// 200 ms MQTT publish that is bridged onto Zenoh.
    #[arg(long, default_value_t = 10)]
    ticks: u32,
}

/// What arrives on the MQTT side: a raw sensor reading.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SensorReading {
    seq: u64,
    celsius: f64,
}

/// Threshold band the transform decides.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
enum AlarmLevel {
    Ok,
    Warn,
    Critical,
}

/// What leaves on the Zenoh side: the transformed decision. Note it is a
/// DIFFERENT type than the ingress payload — the executor item is visibly
/// doing work, not forwarding bytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct AlarmDecision {
    seq: u64,
    celsius: f64,
    fahrenheit: f64,
    level: AlarmLevel,
}

/// The transform. This is the "business logic" the bridge exists to run:
/// a unit conversion AND a threshold decision. Deliberately protocol-blind
/// — it takes a value in, returns a value out, and knows nothing about
/// MQTT or Zenoh.
fn transform(reading: &SensorReading) -> AlarmDecision {
    let fahrenheit = reading.celsius * 9.0 / 5.0 + 32.0;
    let level = if reading.celsius >= CRITICAL_C {
        AlarmLevel::Critical
    } else if reading.celsius >= WARN_C {
        AlarmLevel::Warn
    } else {
        AlarmLevel::Ok
    };
    AlarmDecision {
        seq: reading.seq,
        celsius: reading.celsius,
        fahrenheit,
        level,
    }
}

/// Bridge counters, shared with the summary print at the end of `main`.
#[derive(Default)]
struct Stats {
    ingested: u64,
    bridged: u64,
    egress: u64,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // === 1. INGRESS CONNECTOR: MQTT, backed by MockMqttSession ===========
    //
    // The mock needs no broker address; it loops delivered bytes back to
    // matching subscriptions in-process.
    let mqtt_opts = MqttConnectorOptions::builder()
        .client_id("mqtt-zenoh-bridge")
        .build();
    let mqtt_state = Arc::new(MqttState::new(mqtt_opts));
    let mqtt_session = Arc::new(MockMqttSession::new());
    let mut mqtt = MqttConnector::new(mqtt_state, Arc::clone(&mqtt_session), JsonCodec)?;

    // The MQTT reader is the INGRESS seam. Create it before registering so
    // the session subscription is in place before the first delivery.
    let mqtt_routing = MqttRouting::new(MqttTopic::new(MQTT_TOPIC)?, MqttQos::AtLeastOnce);
    let mqtt_desc =
        ChannelDescriptor::<MqttRouting, N>::new("taktora.examples.bridge.mqtt.in", mqtt_routing)?;
    let ingress = mqtt.create_reader::<SensorReading, N>(&mqtt_desc)?;

    // === 2. EGRESS CONNECTOR: Zenoh, backed by MockZenohSession ==========
    let zenoh_opts = ZenohConnectorOptions::builder()
        .tokio_worker_threads(1)
        .dispatcher_tick(Duration::from_millis(1))
        .build();
    let zenoh_state = Arc::new(ZenohState::new(zenoh_opts));
    let zenoh_session = Arc::new(MockZenohSession::new());
    let mut zenoh = ZenohConnector::new(zenoh_state, Arc::clone(&zenoh_session), JsonCodec)?;

    // The Zenoh writer is the EGRESS seam. A matching Zenoh reader lets the
    // sink item observe what the bridge published (the mock loops it back).
    // Reader before writer so the subscriber attaches before the first send.
    let zenoh_routing = ZenohRouting::new(KeyExprOwned::try_from(ZENOH_KEY)?);
    let zenoh_desc_out = ChannelDescriptor::<ZenohRouting, N>::new(
        "taktora.examples.bridge.zenoh.out",
        zenoh_routing.clone(),
    )?;
    let zenoh_desc_sink = ChannelDescriptor::<ZenohRouting, N>::new(
        "taktora.examples.bridge.zenoh.out",
        zenoh_routing,
    )?;
    let egress_sink = zenoh.create_reader::<AlarmDecision, N>(&zenoh_desc_sink)?;
    let egress = zenoh.create_writer::<AlarmDecision, N>(&zenoh_desc_out)?;

    // === 3. ONE executor, BOTH connectors ===============================
    //
    // Confirmed design fact: two independent connectors register with a
    // single `Executor`. Each spawns its own gateway dispatcher; the
    // executor drives the work items that read/write across them.
    //
    // Two worker threads so the two-hop chain's items (ingress simulator,
    // bridge, egress sink, health) interleave freely rather than contending
    // for a single worker.
    let mut exec = Executor::builder().worker_threads(2).build()?;
    mqtt.register_with(&mut exec)?;
    zenoh.register_with(&mut exec)?;

    // === 4. Health: subscribe to both before adding work items ==========
    let mqtt_health = mqtt.subscribe_health();
    let zenoh_health = zenoh.subscribe_health();
    let mut mqtt_last = mqtt.health().kind();
    let mut zenoh_last = zenoh.health().kind();
    eprintln!("mqtt  connector health at startup: {mqtt_last:?}");
    eprintln!("zenoh connector health at startup: {zenoh_last:?}");

    let stats = Arc::new(Mutex::new(Stats::default()));

    // === 5. INGRESS SIMULATOR ===========================================
    //
    // Stands in for a real MQTT device. Every 200 ms it encodes a
    // `SensorReading` and hands it to `deliver_inbound`, exactly as a
    // broker would deliver a publish to our subscription. Celsius ramps
    // so the alarm decision visibly changes (OK -> WARN -> CRITICAL).
    let total = cli.ticks;
    let mut seq = 0_u64;
    let inject_session = Arc::clone(&mqtt_session);
    let inject_topic = MqttTopic::new(MQTT_TOPIC)?;
    let stats_ingest = Arc::clone(&stats);
    exec.add(item_with_triggers(
        |d| -> Result<(), ExecutorError> {
            d.interval(Duration::from_millis(200));
            Ok(())
        },
        move |_| -> ExecuteResult {
            let celsius = 40.0 + (seq as f64) * 15.0;
            let reading = SensorReading { seq, celsius };
            let mut buf = [0u8; N];
            if let Ok(len) = JsonCodec.encode(&reading, &mut buf) {
                inject_session.deliver_inbound(&inject_topic, &buf[..len]);
                println!("mqtt  in : seq={seq} celsius={celsius:.1}");
                stats_ingest.lock().expect("stats mutex").ingested += 1;
            }
            seq += 1;
            if seq >= u64::from(total) {
                // Done injecting. Return StopChain to retire THIS item only —
                // the executor keeps running the bridge and sink items so the
                // last reading can finish its two-hop journey. The sink item
                // decides when the pipeline is fully drained and stops the
                // executor. (Blocking here instead would starve those items:
                // a worker held in a sleep loop does not let them drain.)
                Ok(ControlFlow::StopChain)
            } else {
                Ok(ControlFlow::Continue)
            }
        },
    ))?;

    // === 6. THE BRIDGE — the whole point ================================
    //
    // Nothing here names MQTT or Zenoh. It drains the ingress reader,
    // transforms, and sends on the egress writer. The SAME two calls —
    // `reader.try_recv()` and `writer.send(...)` — span two protocols.
    let stats_bridge = Arc::clone(&stats);
    exec.add(item_with_triggers(
        |d| -> Result<(), ExecutorError> {
            d.interval(Duration::from_millis(50));
            Ok(())
        },
        move |_| -> ExecuteResult {
            while let Ok(Some(env)) = ingress.try_recv() {
                let decision = transform(&env.value);
                egress
                    .send(&decision)
                    .map_err(|e| -> taktora_executor::ItemError { Box::new(e) })?;
                stats_bridge.lock().expect("stats mutex").bridged += 1;
            }
            Ok(ControlFlow::Continue)
        },
    ))?;

    // === 7. EGRESS SINK =================================================
    //
    // Stands in for a downstream Zenoh subscriber. Drains what the bridge
    // published and prints it, so the round-trip is visible.
    let stats_sink = Arc::clone(&stats);
    let sink_total = u64::from(cli.ticks);
    exec.add(item_with_triggers(
        |d| -> Result<(), ExecutorError> {
            d.interval(Duration::from_millis(50));
            Ok(())
        },
        move |ctx| -> ExecuteResult {
            let mut done = false;
            while let Ok(Some(env)) = egress_sink.try_recv() {
                let d = env.value;
                println!(
                    "zenoh out: seq={} celsius={:.1} fahrenheit={:.1} level={:?}",
                    d.seq, d.celsius, d.fahrenheit, d.level
                );
                let mut s = stats_sink.lock().expect("stats mutex");
                s.egress += 1;
                if s.egress >= sink_total {
                    done = true;
                }
            }
            // Once every injected reading has completed the two-hop journey,
            // this item stops the whole executor — the natural end of the run.
            if done {
                ctx.stop_executor();
            }
            Ok(ControlFlow::Continue)
        },
    ))?;

    // === 8. Health polling for BOTH connectors ==========================
    exec.add(item_with_triggers(
        |d| -> Result<(), ExecutorError> {
            d.interval(Duration::from_millis(250));
            Ok(())
        },
        move |_| -> ExecuteResult {
            while let Ok(Some(event)) = mqtt_health.try_next() {
                let now = event.to.kind();
                if now != mqtt_last {
                    eprintln!("mqtt  connector health: {mqtt_last:?} -> {now:?}");
                    mqtt_last = now;
                }
            }
            while let Ok(Some(event)) = zenoh_health.try_next() {
                let now = event.to.kind();
                if now != zenoh_last {
                    eprintln!("zenoh connector health: {zenoh_last:?} -> {now:?}");
                    zenoh_last = now;
                }
            }
            Ok(ControlFlow::Continue)
        },
    ))?;

    // === 9. Run, then summarize =========================================
    exec.run()?;

    let s = stats.lock().expect("stats mutex");
    println!(
        "ingested={} bridged={} egress={}",
        s.ingested, s.bridged, s.egress
    );
    if s.bridged > 0 && s.egress == 0 {
        return Err(
            "bridge produced decisions but none reached the Zenoh sink (egress == 0); \
             a gateway dispatcher likely failed — check stderr"
                .into(),
        );
    }
    Ok(())
}
