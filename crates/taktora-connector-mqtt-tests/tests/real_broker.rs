//! Broker-in-CI integration tests for the real `rumqttc` backend (M3).
//!
//! Gated behind the `rumqttc-integration` cargo feature; not part of the
//! default test run. Each test **skips** (early-returns) when the broker
//! environment is absent, so the file compiles + noops locally and only
//! really exercises a broker in CI (see `.github/workflows/ci-mqtt.yml`,
//! which stands up mosquitto via `namoshek/mosquitto-github-action`).
//!
//! - `TEST_0255` — username/password CONNECT: accept with the right
//!   credentials, terminal `AuthRejected` with the wrong ones (`REQ_0255`).
//! - `TEST_0257` — plain 1883 JSON pub→broker→sub round-trip through
//!   `RealMqttSession` (`REQ_0257`).
//! - `TEST_0256` — TLS handshake + round-trip on 8883 (`REQ_0256`), gated
//!   additionally on `MQTT_TEST_CA` (the self-signed CA generated in CI).
//!
//! Environment:
//! - `MQTT_TEST_BROKER` — broker host (e.g. `127.0.0.1`); all tests skip if
//!   unset.
//! - `MQTT_TEST_USER` / `MQTT_TEST_PASS` — credentials matching the CI
//!   `mosquitto.passwd` (defaults: `taktora` / `taktora-secret`).
//! - `MQTT_TEST_CA` — path to the PEM CA that signed the broker's 8883
//!   server cert; the TLS test skips if unset.

#![cfg(feature = "rumqttc-integration")]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use taktora_connector_codec::JsonCodec;
use taktora_connector_core::PayloadCodec;
use taktora_connector_mqtt::session::{MqttConnectionState, MqttSessionLike};
use taktora_connector_mqtt::{
    MqttConnectorOptions, MqttQos, MqttRouting, MqttTopic, MqttTopicFilter, RealMqttSession,
};

const PLAIN_PORT: u16 = 1883;
#[cfg(feature = "tls")]
const TLS_PORT: u16 = 8883;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Tick {
    seq: u64,
    label: String,
}

/// The broker host, or `None` (→ skip) when `MQTT_TEST_BROKER` is unset.
fn broker_host() -> Option<String> {
    std::env::var("MQTT_TEST_BROKER")
        .ok()
        .filter(|h| !h.is_empty())
}

fn test_user() -> String {
    std::env::var("MQTT_TEST_USER").unwrap_or_else(|_| "taktora".to_owned())
}

fn test_pass() -> String {
    std::env::var("MQTT_TEST_PASS").unwrap_or_else(|_| "taktora-secret".to_owned())
}

/// Poll `session.state()` until it reaches `Connected` or `deadline` passes.
/// Returns the last observed state.
async fn wait_for_state(
    session: &RealMqttSession,
    want_connected: bool,
    timeout: Duration,
) -> MqttConnectionState {
    let deadline = Instant::now() + timeout;
    let mut state = session.state();
    while Instant::now() < deadline {
        state = session.state();
        let connected = matches!(state, MqttConnectionState::Connected);
        let terminal = matches!(state, MqttConnectionState::AuthRejected { .. });
        if (want_connected && connected) || (!want_connected && terminal) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    state
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_0257_plain_json_round_trip() {
    let Some(host) = broker_host() else {
        eprintln!("skipping test_0257: set MQTT_TEST_BROKER to enable");
        return;
    };

    let opts = MqttConnectorOptions::builder()
        .broker_host(host)
        .broker_port(PLAIN_PORT)
        .client_id("taktora-it-plain")
        .build();
    let session = RealMqttSession::connect(&opts).expect("connect");

    let state = wait_for_state(&session, true, Duration::from_secs(10)).await;
    assert_eq!(state, MqttConnectionState::Connected, "broker CONNACK");

    // Capture inbound payloads through the same InboundRouter seam the
    // gateway installs (ADR_0129).
    let received: Arc<Mutex<Vec<Vec<u8>>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&received);
    session.set_inbound_router(Arc::new(move |_topic, payload: &[u8]| {
        sink.lock().unwrap().push(payload.to_vec());
    }));

    let filter = MqttTopicFilter::new("taktora/it/+/plain").unwrap();
    let _sub = session
        .subscribe(&filter, Box::new(|_: &[u8]| {}))
        .await
        .expect("subscribe");
    // Let the SUBSCRIBE round-trip to the broker before publishing.
    tokio::time::sleep(Duration::from_millis(300)).await;

    let topic = MqttTopic::new("taktora/it/robot7/plain").unwrap();
    let routing = MqttRouting::new(topic, MqttQos::AtLeastOnce);
    let tick = Tick {
        seq: 42,
        label: "hello-mqtt".to_owned(),
    };
    let mut buf = vec![0u8; 256];
    let n = JsonCodec.encode(&tick, &mut buf).unwrap();
    session.publish(&routing, &buf[..n]).await.expect("publish");

    let got = wait_for_payload(&received, Duration::from_secs(10)).await;
    let decoded: Tick = JsonCodec.decode(&got).expect("decode round-tripped JSON");
    assert_eq!(decoded, tick, "payload survives the broker round-trip");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_0255_auth_accept_and_reject() {
    let Some(host) = broker_host() else {
        eprintln!("skipping test_0255: set MQTT_TEST_BROKER to enable");
        return;
    };

    // Accept: correct credentials reach Connected.
    let ok_opts = MqttConnectorOptions::builder()
        .broker_host(host.clone())
        .broker_port(PLAIN_PORT)
        .client_id("taktora-it-auth-ok")
        .credentials(test_user(), test_pass())
        .build();
    let ok_session = RealMqttSession::connect(&ok_opts).expect("connect ok");
    let ok_state = wait_for_state(&ok_session, true, Duration::from_secs(10)).await;
    assert_eq!(
        ok_state,
        MqttConnectionState::Connected,
        "valid credentials accepted on CONNECT"
    );

    // Reject: a bad password drives the terminal AuthRejected state.
    let bad_opts = MqttConnectorOptions::builder()
        .broker_host(host)
        .broker_port(PLAIN_PORT)
        .client_id("taktora-it-auth-bad")
        .credentials(test_user(), "definitely-the-wrong-password")
        .build();
    let bad_session = RealMqttSession::connect(&bad_opts).expect("connect bad");
    let bad_state = wait_for_state(&bad_session, false, Duration::from_secs(10)).await;
    assert!(
        matches!(bad_state, MqttConnectionState::AuthRejected { .. }),
        "bad credentials → terminal AuthRejected, got {bad_state:?}"
    );
}

#[cfg(feature = "tls")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_0256_tls_handshake_round_trip() {
    use taktora_connector_mqtt::TlsOptions;

    let Some(host) = broker_host() else {
        eprintln!("skipping test_0256: set MQTT_TEST_BROKER to enable");
        return;
    };
    let Ok(ca_path) = std::env::var("MQTT_TEST_CA") else {
        eprintln!("skipping test_0256: set MQTT_TEST_CA to the broker CA path");
        return;
    };
    let ca_pem = std::fs::read(&ca_path).expect("read CA pem");

    let opts = MqttConnectorOptions::builder()
        .broker_host(host)
        .broker_port(TLS_PORT)
        .client_id("taktora-it-tls")
        .tls(TlsOptions::new(ca_pem))
        .build();
    let session = RealMqttSession::connect(&opts).expect("connect tls");

    // Reaching Connected proves the rustls handshake on 8883 succeeded.
    let state = wait_for_state(&session, true, Duration::from_secs(15)).await;
    assert_eq!(
        state,
        MqttConnectionState::Connected,
        "TLS handshake completed and CONNACK received"
    );

    // And a message still round-trips over the encrypted link.
    let received: Arc<Mutex<Vec<Vec<u8>>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&received);
    session.set_inbound_router(Arc::new(move |_topic, payload: &[u8]| {
        sink.lock().unwrap().push(payload.to_vec());
    }));
    let filter = MqttTopicFilter::new("taktora/it/tls").unwrap();
    let _sub = session
        .subscribe(&filter, Box::new(|_: &[u8]| {}))
        .await
        .expect("subscribe tls");
    tokio::time::sleep(Duration::from_millis(300)).await;

    let routing = MqttRouting::new(
        MqttTopic::new("taktora/it/tls").unwrap(),
        MqttQos::AtLeastOnce,
    );
    session
        .publish(&routing, b"tls-ok")
        .await
        .expect("publish tls");

    let got = wait_for_payload(&received, Duration::from_secs(10)).await;
    assert_eq!(got, b"tls-ok", "payload survives the TLS round-trip");
}

/// Poll `received` until it holds a payload or `timeout` passes; panics on
/// timeout (a broker was present but never delivered).
async fn wait_for_payload(received: &Arc<Mutex<Vec<Vec<u8>>>>, timeout: Duration) -> Vec<u8> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(payload) = received.lock().unwrap().first().cloned() {
            return payload;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("no inbound payload delivered within {timeout:?}");
}
