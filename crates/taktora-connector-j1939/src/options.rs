//! [`J1939ConnectorOptions`] — typed builder configuring a
//! `J1939Connector` / `J1939Gateway` pair. `BB_0099`, `REQ_0899`.
//!
//! Mirrors `taktora_connector_can::CanConnectorOptions` in shape:
//! per-interface configuration plus bounded bridge capacities and the
//! tokio worker-thread count.
//!
//! ## Extension points for #123 / #125 / #126
//!
//! This tracer-bullet builder deliberately omits the transport-protocol
//! knobs the later issues need. They slot in here without breaking the
//! surface:
//!
//! * **#123 (BAM) / #124 (RTS-CTS):** `tp_timer_*` overrides
//!   (T1..T4 / Tr / Th) and `max_concurrent_tp_sessions`.
//! * **#125 (ETP):** `max_etp_bytes` (defaulting to
//!   [`crate::routing::TP_MAX_LEN`]).
//! * **#126 (address-claim):** the per-interface 64-bit `name` is
//!   already carried by [`J1939Interface`] as a placeholder so the
//!   claim state machine has a NAME to arbitrate with.

use std::time::Duration;

use taktora_connector_can::CanIface;

use crate::addr_claim::DEFAULT_CLAIM_WAIT;
use crate::tp::{DEFAULT_MAX_ETP_BYTES, DEFAULT_MAX_TP_SESSIONS, ETP_MIN_PAYLOAD, TpTimers};

/// Default initial slice length for the ETP large-payload slice channel,
/// in bytes (`ADR_0109`). The publisher's data segment starts here and
/// grows by `AllocationStrategy::PowerOfTwo` up to `max_etp_bytes`. 4096
/// is a small, cache-friendly starting point that still covers the
/// smallest ETP messages (just over 1785 B) without an immediate grow.
pub const DEFAULT_ETP_INITIAL_SLICE_LEN: usize = 4096;

/// Per-interface J1939 configuration.
///
/// Carries the bound CAN interface, this node's source address on that
/// interface, and a 64-bit J1939 NAME placeholder consumed by the
/// address-claim state machine in issue #126 (unused in this tracer
/// bullet beyond being plumbed through).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct J1939Interface {
    /// CAN interface this node binds to.
    pub iface: CanIface,
    /// This node's J1939 source address on `iface`. Used as the TX
    /// source address for outbound frames whose routing leaves
    /// `source_addr = None`.
    pub source_addr: u8,
    /// 64-bit J1939 NAME. Placeholder for the #126 address-claim
    /// arbitration; defaults to `0`.
    pub name: u64,
}

impl J1939Interface {
    /// Default preferred / null J1939 source address (0xFE = "null
    /// address", used before a claim succeeds).
    pub const NULL_ADDRESS: u8 = 0xFE;

    /// Construct an interface config with the given source address and
    /// a zero NAME placeholder.
    #[must_use]
    pub const fn new(iface: CanIface, source_addr: u8) -> Self {
        Self {
            iface,
            source_addr,
            name: 0,
        }
    }

    /// Builder-style 64-bit NAME setter (for #126).
    #[must_use]
    pub const fn with_name(mut self, name: u64) -> Self {
        self.name = name;
        self
    }
}

/// Built `J1939ConnectorOptions`. Constructed via
/// [`J1939ConnectorOptionsBuilder`]; never mutated after build.
#[derive(Clone, Debug)]
pub struct J1939ConnectorOptions {
    interfaces: Vec<J1939Interface>,
    outbound_capacity: usize,
    inbound_capacity: usize,
    inbound_drop_threshold: u64,
    tokio_worker_threads: usize,
    tp_timers: TpTimers,
    max_concurrent_tp_sessions: usize,
    max_etp_bytes: usize,
    etp_initial_slice_len: usize,
    claim_wait: Duration,
}

impl J1939ConnectorOptions {
    /// Start a builder with default values.
    #[must_use]
    pub fn builder() -> J1939ConnectorOptionsBuilder {
        J1939ConnectorOptionsBuilder::new()
    }

    /// Borrow the configured per-interface list.
    #[must_use]
    pub fn interfaces(&self) -> &[J1939Interface] {
        &self.interfaces
    }

    /// Collect just the CAN interfaces (e.g. for the health monitor).
    #[must_use]
    pub fn ifaces(&self) -> Vec<CanIface> {
        self.interfaces.iter().map(|i| i.iface).collect()
    }

    /// Look up the configured source address for `iface`.
    #[must_use]
    pub fn source_addr_for(&self, iface: &CanIface) -> Option<u8> {
        self.interfaces
            .iter()
            .find(|i| &i.iface == iface)
            .map(|i| i.source_addr)
    }

    /// Outbound bridge capacity. Default 256.
    #[must_use]
    pub const fn outbound_capacity(&self) -> usize {
        self.outbound_capacity
    }

    /// Inbound bridge capacity. Default 256.
    #[must_use]
    pub const fn inbound_capacity(&self) -> usize {
        self.inbound_capacity
    }

    /// Cumulative inbound-drop count that triggers a `Degraded`
    /// transition. Default 1.
    #[must_use]
    pub const fn inbound_drop_threshold(&self) -> u64 {
        self.inbound_drop_threshold
    }

    /// Tokio worker-thread count for the gateway sidecar. Default 1.
    #[must_use]
    pub const fn tokio_worker_threads(&self) -> usize {
        self.tokio_worker_threads
    }

    /// J1939-21 transport-protocol timer set (`REQ_0895`). Defaults to
    /// [`TpTimers::default`] (Tr 200, Th 500, T1 750, T2/T3 1250, T4 1050
    /// ms).
    #[must_use]
    pub const fn tp_timers(&self) -> TpTimers {
        self.tp_timers
    }

    /// Maximum concurrent inbound TP sessions per interface (`REQ_0896`).
    /// Default [`DEFAULT_MAX_TP_SESSIONS`] (8). A session opened beyond
    /// this cap is refused with a connection abort.
    #[must_use]
    pub const fn max_concurrent_tp_sessions(&self) -> usize {
        self.max_concurrent_tp_sessions
    }

    /// Maximum ETP reassembly size in bytes (`REQ_0894`/`REQ_0903`,
    /// `ADR_0109`). Bounds both the engine's inbound reassembly buffer and
    /// the slice channel's `max_payload_bytes` ceiling. Default
    /// [`DEFAULT_MAX_ETP_BYTES`] (16 MiB), clamped to a minimum of
    /// [`ETP_MIN_PAYLOAD`] (1786) at build. A session announcing a larger
    /// total is aborted with the J1939 connection-abort reason and
    /// surfaced as a `HealthEvent`.
    #[must_use]
    pub const fn max_etp_bytes(&self) -> usize {
        self.max_etp_bytes
    }

    /// Initial slice length the ETP slice channel's data segment is sized
    /// for; it grows by `AllocationStrategy::PowerOfTwo` up to
    /// [`Self::max_etp_bytes`]. Default [`DEFAULT_ETP_INITIAL_SLICE_LEN`]
    /// (4096), clamped to `1..=max_etp_bytes` at build.
    #[must_use]
    pub const fn etp_initial_slice_len(&self) -> usize {
        self.etp_initial_slice_len
    }

    /// J1939-81 address-claim wait timer (`REQ_0897`): how long an
    /// uncontested claim waits before it is considered `Claimed`. Default
    /// [`DEFAULT_CLAIM_WAIT`] (250 ms). Tests set this small (or large) to
    /// control the Claiming → Up transition deterministically.
    #[must_use]
    pub const fn claim_wait(&self) -> Duration {
        self.claim_wait
    }
}

/// Builder for [`J1939ConnectorOptions`].
#[derive(Debug)]
pub struct J1939ConnectorOptionsBuilder {
    interfaces: Vec<J1939Interface>,
    outbound_capacity: usize,
    inbound_capacity: usize,
    inbound_drop_threshold: u64,
    tokio_worker_threads: usize,
    tp_timers: TpTimers,
    max_concurrent_tp_sessions: usize,
    max_etp_bytes: usize,
    etp_initial_slice_len: usize,
    claim_wait: Duration,
}

impl J1939ConnectorOptionsBuilder {
    /// Construct a builder with default values:
    ///
    /// * `interfaces` — empty; set at least one for a useful gateway.
    /// * `outbound_capacity` / `inbound_capacity` — 256.
    /// * `inbound_drop_threshold` — 1.
    /// * `tokio_worker_threads` — 1.
    /// * `tp_timers` — [`TpTimers::default`] (J1939-21 standard
    ///   defaults).
    /// * `max_concurrent_tp_sessions` — [`DEFAULT_MAX_TP_SESSIONS`] (8).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            interfaces: Vec::new(),
            outbound_capacity: 256,
            inbound_capacity: 256,
            inbound_drop_threshold: 1,
            tokio_worker_threads: 1,
            // `TpTimers::default()` is not const; spell the J1939-21
            // defaults out via the const `Duration::from_millis`.
            tp_timers: TpTimers {
                tr: Duration::from_millis(200),
                th: Duration::from_millis(500),
                t1: Duration::from_millis(750),
                t2: Duration::from_millis(1250),
                t3: Duration::from_millis(1250),
                t4: Duration::from_millis(1050),
            },
            max_concurrent_tp_sessions: DEFAULT_MAX_TP_SESSIONS,
            max_etp_bytes: DEFAULT_MAX_ETP_BYTES,
            etp_initial_slice_len: DEFAULT_ETP_INITIAL_SLICE_LEN,
            claim_wait: DEFAULT_CLAIM_WAIT,
        }
    }

    /// Append a per-interface configuration.
    #[must_use]
    pub fn interface(mut self, interface: J1939Interface) -> Self {
        self.interfaces.push(interface);
        self
    }

    /// Replace the interface list wholesale.
    #[must_use]
    pub fn interfaces(mut self, interfaces: impl IntoIterator<Item = J1939Interface>) -> Self {
        self.interfaces = interfaces.into_iter().collect();
        self
    }

    /// Override outbound bridge capacity. Clamped to `>= 1` at build.
    #[must_use]
    pub const fn outbound_capacity(mut self, n: usize) -> Self {
        self.outbound_capacity = n;
        self
    }

    /// Override inbound bridge capacity. Clamped to `>= 1` at build.
    #[must_use]
    pub const fn inbound_capacity(mut self, n: usize) -> Self {
        self.inbound_capacity = n;
        self
    }

    /// Override the inbound-drop threshold. Clamped to `>= 1` at build.
    #[must_use]
    pub const fn inbound_drop_threshold(mut self, n: u64) -> Self {
        self.inbound_drop_threshold = n;
        self
    }

    /// Override the tokio worker-thread count. Clamped to `>= 1` at
    /// build.
    #[must_use]
    pub const fn tokio_worker_threads(mut self, n: usize) -> Self {
        self.tokio_worker_threads = n;
        self
    }

    /// Override the J1939-21 transport-protocol timer set (`REQ_0895`).
    #[must_use]
    pub const fn tp_timers(mut self, timers: TpTimers) -> Self {
        self.tp_timers = timers;
        self
    }

    /// Override the per-interface concurrent inbound-TP-session cap
    /// (`REQ_0896`). Clamped to `>= 1` at build.
    #[must_use]
    pub const fn max_concurrent_tp_sessions(mut self, n: usize) -> Self {
        self.max_concurrent_tp_sessions = n;
        self
    }

    /// Override the ETP reassembly ceiling (`REQ_0894`/`REQ_0903`).
    /// Clamped to a minimum of [`ETP_MIN_PAYLOAD`] (1786) at build so a
    /// too-small cap can never make ETP impossible. Set this small (e.g.
    /// 8192) in tests to exercise the oversize abort.
    #[must_use]
    pub const fn max_etp_bytes(mut self, n: usize) -> Self {
        self.max_etp_bytes = n;
        self
    }

    /// Override the ETP slice channel's initial slice length. Clamped to
    /// `1..=max_etp_bytes` at build.
    #[must_use]
    pub const fn etp_initial_slice_len(mut self, n: usize) -> Self {
        self.etp_initial_slice_len = n;
        self
    }

    /// Override the J1939-81 address-claim wait timer (`REQ_0897`). Default
    /// [`DEFAULT_CLAIM_WAIT`] (250 ms).
    #[must_use]
    pub const fn claim_wait(mut self, wait: Duration) -> Self {
        self.claim_wait = wait;
        self
    }

    /// Finalise, applying the `>= 1` clamps.
    #[must_use]
    pub fn build(self) -> J1939ConnectorOptions {
        let max_etp_bytes = self.max_etp_bytes.max(ETP_MIN_PAYLOAD);
        J1939ConnectorOptions {
            interfaces: self.interfaces,
            outbound_capacity: self.outbound_capacity.max(1),
            inbound_capacity: self.inbound_capacity.max(1),
            inbound_drop_threshold: self.inbound_drop_threshold.max(1),
            tokio_worker_threads: self.tokio_worker_threads.max(1),
            tp_timers: self.tp_timers,
            max_concurrent_tp_sessions: self.max_concurrent_tp_sessions.max(1),
            max_etp_bytes,
            etp_initial_slice_len: self.etp_initial_slice_len.max(1).min(max_etp_bytes),
            claim_wait: self.claim_wait,
        }
    }
}

impl Default for J1939ConnectorOptionsBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iface(name: &str) -> CanIface {
        CanIface::new(name).unwrap()
    }

    #[test]
    fn defaults_and_clamps() {
        let opts = J1939ConnectorOptions::builder()
            .outbound_capacity(0)
            .inbound_capacity(0)
            .inbound_drop_threshold(0)
            .tokio_worker_threads(0)
            .build();
        assert_eq!(opts.outbound_capacity(), 1);
        assert_eq!(opts.inbound_capacity(), 1);
        assert_eq!(opts.inbound_drop_threshold(), 1);
        assert_eq!(opts.tokio_worker_threads(), 1);
        assert!(opts.interfaces().is_empty());
    }

    #[test]
    fn interfaces_and_source_addr_lookup() {
        let opts = J1939ConnectorOptions::builder()
            .interface(J1939Interface::new(iface("vcan0"), 0x11))
            .interface(J1939Interface::new(iface("vcan1"), 0x22).with_name(0xDEAD_BEEF))
            .build();
        assert_eq!(opts.interfaces().len(), 2);
        assert_eq!(opts.source_addr_for(&iface("vcan0")), Some(0x11));
        assert_eq!(opts.source_addr_for(&iface("vcan1")), Some(0x22));
        assert_eq!(opts.source_addr_for(&iface("vcan9")), None);
        assert_eq!(opts.ifaces().len(), 2);
    }
}
