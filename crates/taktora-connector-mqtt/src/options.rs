//! [`MqttConnectorOptions`] — typed builder configuring an
//! `MqttConnector` / gateway pair. `REQ_0259` (bounded, configurable
//! bridge capacities), `REQ_0255` (credentials), `REQ_0981`/`REQ_0983`
//! (reconnect backoff + attempt ceiling), `REQ_0984` (clean session).

use std::time::Duration;

/// Default MQTT broker TCP port (unencrypted, MQTT 3.1.1).
pub const DEFAULT_BROKER_PORT: u16 = 1883;
/// Default client id presented on CONNECT.
pub const DEFAULT_CLIENT_ID: &str = "taktora-mqtt";
/// Default keep-alive interval.
pub const DEFAULT_KEEP_ALIVE: Duration = Duration::from_secs(60);
/// Default bounded bridge capacity for both directions (`REQ_0259`).
pub const DEFAULT_BRIDGE_CAPACITY: usize = 64;

/// Optional username/password credentials presented on the MQTT CONNECT
/// packet (`REQ_0255`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credentials {
    /// Username.
    pub username: String,
    /// Password.
    pub password: String,
}

/// Optional TLS configuration for the broker connection (`REQ_0256`).
///
/// Carries only the PEM-encoded CA certificate(s) used to validate the
/// broker; client-certificate authentication is deferred to a follow-on
/// spec. The bytes are stored raw (no `rustls` types) so this type — and
/// [`MqttConnectorOptions`] — compile in the lean default build; the real
/// session interprets them into a rumqttc TLS transport only under the
/// `tls` cargo feature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlsOptions {
    ca_cert_pem: Vec<u8>,
}

impl TlsOptions {
    /// Construct TLS options from PEM-encoded CA certificate(s).
    #[must_use]
    pub fn new(ca_cert_pem: impl Into<Vec<u8>>) -> Self {
        Self {
            ca_cert_pem: ca_cert_pem.into(),
        }
    }

    /// The PEM-encoded CA certificate(s) validating the broker.
    #[must_use]
    pub fn ca_cert_pem(&self) -> &[u8] {
        &self.ca_cert_pem
    }
}

/// Process-wide configuration for an MQTT connector. Constructed via
/// [`MqttConnectorOptions::builder`]; never mutated after `build`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MqttConnectorOptions {
    broker_host: String,
    broker_port: u16,
    client_id: String,
    keep_alive: Duration,
    clean_session: bool,
    credentials: Option<Credentials>,
    tls: Option<TlsOptions>,
    outbound_bridge_capacity: usize,
    inbound_bridge_capacity: usize,
    inbound_drop_threshold: u64,
    reconnect_initial_backoff: Duration,
    reconnect_max_backoff: Duration,
    reconnect_attempt_ceiling: u32,
}

impl MqttConnectorOptions {
    /// Start a builder with default values.
    #[must_use]
    pub fn builder() -> MqttConnectorOptionsBuilder {
        MqttConnectorOptionsBuilder::new()
    }

    /// Broker host (`REQ_0255`). Default `"localhost"`.
    #[must_use]
    pub fn broker_host(&self) -> &str {
        &self.broker_host
    }

    /// Broker TCP port. Default [`DEFAULT_BROKER_PORT`].
    #[must_use]
    pub const fn broker_port(&self) -> u16 {
        self.broker_port
    }

    /// Client id presented on CONNECT. Default [`DEFAULT_CLIENT_ID`].
    #[must_use]
    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    /// Keep-alive interval. Default [`DEFAULT_KEEP_ALIVE`].
    #[must_use]
    pub const fn keep_alive(&self) -> Duration {
        self.keep_alive
    }

    /// Clean-session flag (`REQ_0984`). Default `true`.
    #[must_use]
    pub const fn clean_session(&self) -> bool {
        self.clean_session
    }

    /// Optional credentials (`REQ_0255`). Default `None`.
    #[must_use]
    pub const fn credentials(&self) -> Option<&Credentials> {
        self.credentials.as_ref()
    }

    /// Optional TLS configuration (`REQ_0256`). Default `None` (plain TCP).
    /// Honoured by the real session only under the `tls` cargo feature.
    #[must_use]
    pub const fn tls(&self) -> Option<&TlsOptions> {
        self.tls.as_ref()
    }

    /// Outbound (plugin → gateway) bridge capacity (`REQ_0259`).
    /// Default [`DEFAULT_BRIDGE_CAPACITY`]; clamped to at least 1.
    #[must_use]
    pub const fn outbound_bridge_capacity(&self) -> usize {
        self.outbound_bridge_capacity
    }

    /// Inbound (gateway → plugin) bridge capacity (`REQ_0259`).
    /// Default [`DEFAULT_BRIDGE_CAPACITY`]; clamped to at least 1.
    #[must_use]
    pub const fn inbound_bridge_capacity(&self) -> usize {
        self.inbound_bridge_capacity
    }

    /// Cumulative inbound-drop count that triggers a single
    /// `Degraded` transition (`REQ_0261`). Default 1; clamped to at least 1.
    #[must_use]
    pub const fn inbound_drop_threshold(&self) -> u64 {
        self.inbound_drop_threshold
    }

    /// Initial reconnect backoff (`REQ_0981`). Default 100 ms.
    #[must_use]
    pub const fn reconnect_initial_backoff(&self) -> Duration {
        self.reconnect_initial_backoff
    }

    /// Maximum reconnect backoff (`REQ_0981`). Default 30 s.
    #[must_use]
    pub const fn reconnect_max_backoff(&self) -> Duration {
        self.reconnect_max_backoff
    }

    /// Consecutive-failed-reconnect ceiling before the connector goes
    /// `Down` (`REQ_0983`). Default 10.
    #[must_use]
    pub const fn reconnect_attempt_ceiling(&self) -> u32 {
        self.reconnect_attempt_ceiling
    }
}

/// Builder for [`MqttConnectorOptions`].
#[derive(Debug, Clone)]
pub struct MqttConnectorOptionsBuilder {
    broker_host: String,
    broker_port: u16,
    client_id: String,
    keep_alive: Duration,
    clean_session: bool,
    credentials: Option<Credentials>,
    tls: Option<TlsOptions>,
    outbound_bridge_capacity: usize,
    inbound_bridge_capacity: usize,
    inbound_drop_threshold: u64,
    reconnect_initial_backoff: Duration,
    reconnect_max_backoff: Duration,
    reconnect_attempt_ceiling: u32,
}

impl MqttConnectorOptionsBuilder {
    /// Construct a builder with default values.
    #[must_use]
    pub fn new() -> Self {
        Self {
            broker_host: "localhost".to_string(),
            broker_port: DEFAULT_BROKER_PORT,
            client_id: DEFAULT_CLIENT_ID.to_string(),
            keep_alive: DEFAULT_KEEP_ALIVE,
            clean_session: true,
            credentials: None,
            tls: None,
            outbound_bridge_capacity: DEFAULT_BRIDGE_CAPACITY,
            inbound_bridge_capacity: DEFAULT_BRIDGE_CAPACITY,
            inbound_drop_threshold: 1,
            reconnect_initial_backoff: Duration::from_millis(100),
            reconnect_max_backoff: Duration::from_secs(30),
            reconnect_attempt_ceiling: 10,
        }
    }

    /// Set the broker host.
    #[must_use]
    pub fn broker_host(mut self, host: impl Into<String>) -> Self {
        self.broker_host = host.into();
        self
    }

    /// Set the broker port.
    #[must_use]
    pub const fn broker_port(mut self, port: u16) -> Self {
        self.broker_port = port;
        self
    }

    /// Set the client id.
    #[must_use]
    pub fn client_id(mut self, id: impl Into<String>) -> Self {
        self.client_id = id.into();
        self
    }

    /// Set the keep-alive interval.
    #[must_use]
    pub const fn keep_alive(mut self, d: Duration) -> Self {
        self.keep_alive = d;
        self
    }

    /// Set the clean-session flag (`REQ_0984`).
    #[must_use]
    pub const fn clean_session(mut self, clean: bool) -> Self {
        self.clean_session = clean;
        self
    }

    /// Set username/password credentials (`REQ_0255`).
    #[must_use]
    pub fn credentials(mut self, username: impl Into<String>, password: impl Into<String>) -> Self {
        self.credentials = Some(Credentials {
            username: username.into(),
            password: password.into(),
        });
        self
    }

    /// Set the TLS configuration (`REQ_0256`). Honoured by the real session
    /// only under the `tls` cargo feature.
    #[must_use]
    pub fn tls(mut self, tls: TlsOptions) -> Self {
        self.tls = Some(tls);
        self
    }

    /// Set the outbound bridge capacity (`REQ_0259`). Clamped to at least
    /// 1 at build time.
    #[must_use]
    pub const fn outbound_bridge_capacity(mut self, n: usize) -> Self {
        self.outbound_bridge_capacity = n;
        self
    }

    /// Set the inbound bridge capacity (`REQ_0259`). Clamped to at least 1
    /// at build time.
    #[must_use]
    pub const fn inbound_bridge_capacity(mut self, n: usize) -> Self {
        self.inbound_bridge_capacity = n;
        self
    }

    /// Set the inbound-drop threshold (`REQ_0261`). Clamped to at least 1.
    #[must_use]
    pub const fn inbound_drop_threshold(mut self, n: u64) -> Self {
        self.inbound_drop_threshold = n;
        self
    }

    /// Set the initial reconnect backoff (`REQ_0981`).
    #[must_use]
    pub const fn reconnect_initial_backoff(mut self, d: Duration) -> Self {
        self.reconnect_initial_backoff = d;
        self
    }

    /// Set the maximum reconnect backoff (`REQ_0981`).
    #[must_use]
    pub const fn reconnect_max_backoff(mut self, d: Duration) -> Self {
        self.reconnect_max_backoff = d;
        self
    }

    /// Set the reconnect-attempt ceiling (`REQ_0983`).
    #[must_use]
    pub const fn reconnect_attempt_ceiling(mut self, n: u32) -> Self {
        self.reconnect_attempt_ceiling = n;
        self
    }

    /// Finalise, applying the capacity/threshold clamps (at least 1).
    #[must_use]
    pub fn build(self) -> MqttConnectorOptions {
        MqttConnectorOptions {
            broker_host: self.broker_host,
            broker_port: self.broker_port,
            client_id: self.client_id,
            keep_alive: self.keep_alive,
            clean_session: self.clean_session,
            credentials: self.credentials,
            tls: self.tls,
            outbound_bridge_capacity: self.outbound_bridge_capacity.max(1),
            inbound_bridge_capacity: self.inbound_bridge_capacity.max(1),
            inbound_drop_threshold: self.inbound_drop_threshold.max(1),
            reconnect_initial_backoff: self.reconnect_initial_backoff,
            reconnect_max_backoff: self.reconnect_max_backoff,
            reconnect_attempt_ceiling: self.reconnect_attempt_ceiling,
        }
    }
}

impl Default for MqttConnectorOptionsBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults() {
        let o = MqttConnectorOptions::builder().build();
        assert_eq!(o.broker_host(), "localhost");
        assert_eq!(o.broker_port(), DEFAULT_BROKER_PORT);
        assert_eq!(o.client_id(), DEFAULT_CLIENT_ID);
        assert_eq!(o.keep_alive(), DEFAULT_KEEP_ALIVE);
        assert!(o.clean_session());
        assert!(o.credentials().is_none());
        assert!(o.tls().is_none(), "TLS defaults to off (plain TCP)");
        assert_eq!(o.outbound_bridge_capacity(), DEFAULT_BRIDGE_CAPACITY);
        assert_eq!(o.inbound_bridge_capacity(), DEFAULT_BRIDGE_CAPACITY);
        assert_eq!(o.inbound_drop_threshold(), 1);
        assert_eq!(o.reconnect_attempt_ceiling(), 10);
    }

    #[test]
    fn clamps_zero_capacities_and_threshold() {
        // REQ_0259: capacities are bounded and must be a usable channel size.
        let o = MqttConnectorOptions::builder()
            .outbound_bridge_capacity(0)
            .inbound_bridge_capacity(0)
            .inbound_drop_threshold(0)
            .build();
        assert_eq!(o.outbound_bridge_capacity(), 1);
        assert_eq!(o.inbound_bridge_capacity(), 1);
        assert_eq!(o.inbound_drop_threshold(), 1);
    }

    #[test]
    fn overrides_round_trip() {
        let o = MqttConnectorOptions::builder()
            .broker_host("broker.example.com")
            .broker_port(8883)
            .client_id("robot-7")
            .clean_session(false)
            .credentials("user", "secret")
            .outbound_bridge_capacity(128)
            .inbound_bridge_capacity(256)
            .reconnect_attempt_ceiling(3)
            .build();
        assert_eq!(o.broker_host(), "broker.example.com");
        assert_eq!(o.broker_port(), 8883);
        assert_eq!(o.client_id(), "robot-7");
        assert!(!o.clean_session());
        assert_eq!(
            o.credentials(),
            Some(&Credentials {
                username: "user".to_string(),
                password: "secret".to_string(),
            })
        );
        assert_eq!(o.outbound_bridge_capacity(), 128);
        assert_eq!(o.inbound_bridge_capacity(), 256);
        assert_eq!(o.reconnect_attempt_ceiling(), 3);
    }

    #[test]
    fn tls_options_round_trip() {
        // REQ_0256: TLS CA material survives the builder as raw PEM bytes.
        const CA: &[u8] = b"-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----\n";
        let o = MqttConnectorOptions::builder()
            .broker_port(8883)
            .tls(TlsOptions::new(CA))
            .build();
        assert_eq!(o.tls().map(TlsOptions::ca_cert_pem), Some(CA));
    }
}
