//! `RealMqttSession` — [`MqttSessionLike`] over `rumqttc` 0.24 (MQTT 3.1.1,
//! `REQ_0257`).
//!
//! Compiled only with the `rumqttc-integration` cargo feature; the always-
//! built [`crate::mock::MockMqttSession`] stays the default backend so a
//! plain `cargo build` never pulls rumqttc's transitive stack.
//!
//! # Reconnect ownership (`ADR_0128`)
//!
//! `rumqttc`'s `EventLoop` owns reconnection: each `poll()` re-establishes
//! the TCP+MQTT session on its own schedule after an error. This session
//! does **not** implement its own reconnect — it spawns a *pump* task that
//! polls the event loop in a loop and maps poll outcomes onto the observable
//! [`MqttConnectionState`] the gateway's health watcher reads:
//!
//! * a successful `ConnAck` → [`MqttConnectionState::Connected`], reconnect
//!   counter reset (`REQ_0980`);
//! * a connection error → [`MqttConnectionState::Connecting`], reconnect
//!   counter incremented, then a bounded backoff sleep before the next poll
//!   (`REQ_0981`, `REQ_0983`);
//! * an auth-rejected `ConnAck` (surfaced by rumqttc as
//!   `ConnectionError::ConnectionRefused`) → terminal
//!   [`MqttConnectionState::AuthRejected`]; the pump stops (`REQ_0982`).
//!
//! Each inbound `Publish` is routed through the gateway's single
//! [`InboundRouter`] (`ADR_0129`), exactly as the mock does via
//! `deliver_inbound` — the per-subscription sink is unused because the
//! gateway runs its own wildcard demux.
//!
//! # Runtime
//!
//! The pump is spawned with [`tokio::spawn`], so [`RealMqttSession::connect`]
//! must be awaited inside a tokio runtime (the gateway runtime in
//! production, the test runtime in the integration suite). Tokio never leaks
//! into taktora-executor's WaitSet thread (`REQ_0258`) — the pump and the
//! `AsyncClient`→`EventLoop` request channel are entirely gateway-side.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use rumqttc::{
    AsyncClient, ConnectReturnCode, ConnectionError, Event, EventLoop, MqttOptions, Packet, QoS,
};
use tokio::task::JoinHandle;
use tracing::{debug, warn};

use crate::options::MqttConnectorOptions;
use crate::routing::{MqttQos, MqttRouting};
use crate::session::{
    InboundRouter, MqttConnectionState, MqttSessionLike, PayloadSink, SessionError,
    SubscriptionHandle,
};
use crate::topic::{MqttTopic, MqttTopicFilter};

/// Bounded capacity for the `AsyncClient` → `EventLoop` request channel.
const REQUEST_CHANNEL_CAP: usize = 128;

/// Pump-updated state shared between [`RealMqttSession`] and its event-loop
/// pump task. The gateway health watcher reads [`Self::state`] /
/// [`Self::reconnect_attempts`]; the pump routes inbound PUBLISHes through
/// the installed [`InboundRouter`].
struct Shared {
    state: RwLock<MqttConnectionState>,
    reconnect_attempts: AtomicU32,
    inbound_router: Mutex<Option<InboundRouter>>,
}

impl Shared {
    fn new() -> Self {
        Self {
            state: RwLock::new(MqttConnectionState::Connecting),
            reconnect_attempts: AtomicU32::new(0),
            inbound_router: Mutex::new(None),
        }
    }

    fn set_state(&self, state: MqttConnectionState) {
        *self.state.write().expect("real session state lock not poisoned") = state;
    }

    fn snapshot_state(&self) -> MqttConnectionState {
        self.state
            .read()
            .expect("real session state lock not poisoned")
            .clone()
    }

    /// Apply a successful `ConnAck`: mark connected and reset the reconnect
    /// counter (`REQ_0980`). rumqttc only surfaces success codes as an
    /// `Ok(ConnAck)`; a rejected code arrives as a poll `Err` instead, so
    /// the non-success branch here is defensive only.
    fn on_connack(&self, code: ConnectReturnCode) {
        if matches!(code, ConnectReturnCode::Success) {
            self.reconnect_attempts.store(0, Ordering::Release);
            self.set_state(MqttConnectionState::Connected);
        } else {
            self.set_state(MqttConnectionState::AuthRejected {
                reason: format!("{code:?}"),
            });
        }
    }

    /// Route one inbound PUBLISH through the installed demux router
    /// (`ADR_0129`, `REQ_0987`). A no-op if no router is installed or the
    /// broker-supplied topic fails validation.
    fn route_publish(&self, topic: &str, payload: &[u8]) {
        let router = self
            .inbound_router
            .lock()
            .expect("real session router lock not poisoned")
            .clone();
        if let (Some(router), Ok(topic)) = (router, MqttTopic::new(topic)) {
            router(&topic, payload);
        }
    }

    /// Record a transient connection error: bump the reconnect counter and
    /// enter [`MqttConnectionState::Connecting`] (`REQ_0981`, `REQ_0983`).
    fn on_connection_error(&self, err: &ConnectionError) {
        self.reconnect_attempts.fetch_add(1, Ordering::AcqRel);
        self.set_state(MqttConnectionState::Connecting);
        debug!(error = %err, "mqtt event-loop connection error; reconnecting");
    }
}

/// [`MqttSessionLike`] over the real rumqttc 0.24 stack (`REQ_0257`).
///
/// Holds the `AsyncClient` (used by `publish` / `subscribe`), the shared
/// pump-updated state, and the pump [`JoinHandle`] (aborted on `Drop`).
pub struct RealMqttSession {
    client: AsyncClient,
    shared: Arc<Shared>,
    pump: Mutex<Option<JoinHandle<()>>>,
}

impl std::fmt::Debug for RealMqttSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RealMqttSession")
            .field("state", &self.shared.snapshot_state())
            .finish_non_exhaustive()
    }
}

impl RealMqttSession {
    /// Connect to the broker described by `opts` and spawn the event-loop
    /// pump task (`ADR_0128`).
    ///
    /// Must be awaited inside a tokio runtime — the pump is spawned with
    /// [`tokio::spawn`]. Returns immediately with the session in
    /// [`MqttConnectionState::Connecting`]; the pump flips it to
    /// `Connected` once the broker's `ConnAck` arrives.
    ///
    /// # Errors
    ///
    /// [`SessionError::ConnectFailed`] if the options cannot be translated
    /// into a valid `rumqttc::MqttOptions` (e.g. TLS was requested without
    /// the `tls` feature).
    pub fn connect(opts: &MqttConnectorOptions) -> Result<Self, SessionError> {
        let mqtt_options = build_mqtt_options(opts)?;
        let (client, event_loop) = AsyncClient::new(mqtt_options, REQUEST_CHANNEL_CAP);
        let shared = Arc::new(Shared::new());
        let pump = tokio::spawn(run_pump(
            Arc::clone(&shared),
            event_loop,
            opts.reconnect_initial_backoff(),
            opts.reconnect_max_backoff(),
        ));
        Ok(Self {
            client,
            shared,
            pump: Mutex::new(Some(pump)),
        })
    }
}

impl Drop for RealMqttSession {
    fn drop(&mut self) {
        // Take the handle out (releasing the guard) before touching it, so
        // the MutexGuard's significant Drop does not span the `if let`.
        let handle = self
            .pump
            .lock()
            .expect("real session pump lock not poisoned")
            .take();
        if let Some(handle) = handle {
            handle.abort();
        }
    }
}

impl MqttSessionLike for RealMqttSession {
    fn state(&self) -> MqttConnectionState {
        self.shared.snapshot_state()
    }

    fn reconnect_attempts(&self) -> u32 {
        self.shared.reconnect_attempts.load(Ordering::Acquire)
    }

    async fn publish(&self, routing: &MqttRouting, payload: &[u8]) -> Result<(), SessionError> {
        let topic = routing.topic().as_str().to_owned();
        let qos = qos_to_rumqttc(routing.qos());
        self.client
            .publish(topic, qos, routing.retained(), payload.to_vec())
            .await
            .map_err(|e| SessionError::PublishFailed {
                reason: e.to_string(),
            })
    }

    async fn subscribe(
        &self,
        filter: &MqttTopicFilter,
        _sink: PayloadSink,
    ) -> Result<SubscriptionHandle, SessionError> {
        // The per-subscription `sink` is intentionally unused: inbound
        // delivery runs through the gateway's single `InboundRouter`
        // (`ADR_0129`), so a broker SUBSCRIBE only registers wire interest.
        let filter_str = filter.as_str().to_owned();
        self.client
            .subscribe(filter_str.clone(), QoS::AtLeastOnce)
            .await
            .map_err(|e| SessionError::SubscribeFailed {
                reason: e.to_string(),
            })?;
        let guard = RealSubscription {
            client: self.client.clone(),
            filter: filter_str,
        };
        Ok(SubscriptionHandle(Box::new(guard)))
    }

    fn set_inbound_router(&self, router: InboundRouter) {
        *self
            .shared
            .inbound_router
            .lock()
            .expect("real session router lock not poisoned") = Some(router);
    }
}

/// Drop guard inside a [`SubscriptionHandle`]: sends UNSUBSCRIBE when the
/// last handle for a filter is dropped (`REQ_0986`). `try_unsubscribe` is
/// the sync, non-blocking variant, safe to call from `Drop`.
struct RealSubscription {
    client: AsyncClient,
    filter: String,
}

impl Drop for RealSubscription {
    fn drop(&mut self) {
        if let Err(e) = self.client.try_unsubscribe(self.filter.clone()) {
            warn!(filter = %self.filter, error = %e, "mqtt UNSUBSCRIBE failed");
        }
    }
}

/// Translate an [`MqttConnectorOptions`] into a `rumqttc::MqttOptions`,
/// presenting credentials (`REQ_0255`) and — under the `tls` feature — a
/// rustls TLS transport (`REQ_0256`).
fn build_mqtt_options(opts: &MqttConnectorOptions) -> Result<MqttOptions, SessionError> {
    let mut mqtt_options = MqttOptions::new(
        opts.client_id().to_owned(),
        opts.broker_host().to_owned(),
        opts.broker_port(),
    );
    mqtt_options.set_keep_alive(opts.keep_alive());
    mqtt_options.set_clean_session(opts.clean_session());
    if let Some(creds) = opts.credentials() {
        mqtt_options.set_credentials(creds.username.clone(), creds.password.clone());
    }
    apply_transport(&mut mqtt_options, opts)?;
    Ok(mqtt_options)
}

/// Wire the TLS transport when TLS options are configured and the `tls`
/// feature is enabled (`REQ_0256`). Plain TCP otherwise.
#[cfg(feature = "tls")]
fn apply_transport(
    mqtt_options: &mut MqttOptions,
    opts: &MqttConnectorOptions,
) -> Result<(), SessionError> {
    use rumqttc::{TlsConfiguration, Transport};
    let Some(tls) = opts.tls() else {
        return Ok(());
    };
    let ca = tls.ca_cert_pem();
    if ca.is_empty() {
        return Err(SessionError::ConnectFailed {
            reason: "TLS configured with an empty CA certificate".to_owned(),
        });
    }
    mqtt_options.set_transport(Transport::Tls(TlsConfiguration::Simple {
        ca: ca.to_vec(),
        alpn: None,
        client_auth: None,
    }));
    Ok(())
}

/// Non-TLS build: honour plain TCP and reject a TLS request that cannot be
/// served because the `tls` feature is off (`REQ_0256`).
#[cfg(not(feature = "tls"))]
fn apply_transport(
    _mqtt_options: &mut MqttOptions,
    opts: &MqttConnectorOptions,
) -> Result<(), SessionError> {
    match opts.tls() {
        Some(_) => Err(SessionError::ConnectFailed {
            reason: "TLS configured but the `tls` cargo feature is not enabled".to_owned(),
        }),
        None => Ok(()),
    }
}

/// Map the connector's [`MqttQos`] onto rumqttc's `QoS` (QoS 0 / 1 only,
/// `REQ_0252`).
const fn qos_to_rumqttc(qos: MqttQos) -> QoS {
    match qos {
        MqttQos::AtMostOnce => QoS::AtMostOnce,
        MqttQos::AtLeastOnce => QoS::AtLeastOnce,
    }
}

/// Event-loop pump (`ADR_0128`). Polls the event loop, maps outcomes onto
/// the shared connection state, and routes inbound PUBLISHes. Exits on a
/// terminal auth-reject or when aborted on session `Drop`.
async fn run_pump(
    shared: Arc<Shared>,
    mut event_loop: EventLoop,
    initial_backoff: Duration,
    max_backoff: Duration,
) {
    let mut backoff = initial_backoff;
    loop {
        match event_loop.poll().await {
            Ok(event) => {
                apply_event(&shared, &event);
                backoff = initial_backoff;
            }
            Err(ConnectionError::ConnectionRefused(code)) => {
                shared.set_state(MqttConnectionState::AuthRejected {
                    reason: format!("{code:?}"),
                });
                warn!(?code, "mqtt CONNACK rejected; connector going Down");
                break;
            }
            Err(err) => {
                shared.on_connection_error(&err);
                tokio::time::sleep(backoff).await;
                backoff = backoff.saturating_mul(2).min(max_backoff);
            }
        }
    }
}

/// Apply one successfully-polled event to the shared state: reflect a
/// `ConnAck` and route a `Publish`; ignore everything else.
fn apply_event(shared: &Shared, event: &Event) {
    if let Event::Incoming(packet) = event {
        match packet {
            Packet::ConnAck(ack) => shared.on_connack(ack.code),
            Packet::Publish(publish) => shared.route_publish(&publish.topic, &publish.payload),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::TlsOptions;

    #[test]
    fn build_mqtt_options_maps_credentials_keepalive_clean_session() {
        // REQ_0255: username/password are presented on CONNECT.
        // REQ_0984: clean-session flag survives; keep-alive survives.
        let opts = MqttConnectorOptions::builder()
            .broker_host("broker.example.com")
            .broker_port(1883)
            .client_id("robot-7")
            .clean_session(false)
            .keep_alive(Duration::from_secs(17))
            .credentials("user", "secret")
            .build();
        let mqtt = build_mqtt_options(&opts).expect("plain TCP options build");
        assert_eq!(
            mqtt.broker_address(),
            ("broker.example.com".to_owned(), 1883)
        );
        assert_eq!(
            mqtt.credentials(),
            Some(("user".to_owned(), "secret".to_owned()))
        );
        assert!(!mqtt.clean_session());
        assert_eq!(mqtt.keep_alive(), Duration::from_secs(17));
    }

    #[test]
    fn qos_mapping_is_exhaustive_and_stable() {
        // REQ_0252: QoS 0 and 1 map to the matching rumqttc levels.
        assert_eq!(qos_to_rumqttc(MqttQos::AtMostOnce), QoS::AtMostOnce);
        assert_eq!(qos_to_rumqttc(MqttQos::AtLeastOnce), QoS::AtLeastOnce);
    }

    #[test]
    #[cfg(not(feature = "tls"))]
    fn tls_without_feature_is_rejected() {
        // REQ_0256: requesting TLS without the `tls` feature is a clear error.
        let opts = MqttConnectorOptions::builder()
            .tls(TlsOptions::new(b"-----BEGIN CERTIFICATE-----".to_vec()))
            .build();
        let err = build_mqtt_options(&opts).expect_err("TLS without feature must fail");
        assert!(matches!(err, SessionError::ConnectFailed { .. }));
    }

    #[test]
    #[cfg(feature = "tls")]
    fn tls_transport_is_wired_when_feature_enabled() {
        // REQ_0256: with the `tls` feature and CA material, the transport
        // becomes TLS. A syntactically-empty CA still builds MqttOptions —
        // validation happens at handshake time inside rumqttc.
        let opts = MqttConnectorOptions::builder()
            .broker_port(8883)
            .tls(TlsOptions::new(b"-----BEGIN CERTIFICATE-----\n".to_vec()))
            .build();
        let mqtt = build_mqtt_options(&opts).expect("tls options build");
        assert!(matches!(mqtt.transport(), rumqttc::Transport::Tls(_)));
    }

    #[test]
    fn shared_state_transitions_track_connack_and_errors() {
        // REQ_0980/REQ_0983: ConnAck → Connected + counter reset; a
        // connection error → Connecting + counter bump.
        let shared = Shared::new();
        assert_eq!(shared.snapshot_state(), MqttConnectionState::Connecting);

        shared.on_connection_error(&ConnectionError::NetworkTimeout);
        assert_eq!(shared.reconnect_attempts.load(Ordering::Acquire), 1);
        assert_eq!(shared.snapshot_state(), MqttConnectionState::Connecting);

        shared.on_connack(ConnectReturnCode::Success);
        assert_eq!(shared.snapshot_state(), MqttConnectionState::Connected);
        assert_eq!(
            shared.reconnect_attempts.load(Ordering::Acquire),
            0,
            "a successful ConnAck resets the reconnect counter"
        );

        shared.on_connack(ConnectReturnCode::NotAuthorized);
        assert!(matches!(
            shared.snapshot_state(),
            MqttConnectionState::AuthRejected { .. }
        ));
    }
}
