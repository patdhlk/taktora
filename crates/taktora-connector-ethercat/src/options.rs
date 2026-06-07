//! [`EthercatConnectorOptions`] — typed builder configuring an
//! `EthercatConnector` / `EthercatGateway` pair. `ADR_0027`.
//!
//! The PDO mapping is declared as `&'static [SubDeviceMap]` so it
//! lives in `.rodata` and the gateway needs no per-instance heap for
//! it (`REQ_0314`, `REQ_0315`).

use std::sync::Arc;
use std::time::Duration;

use taktora_connector_core::{ExponentialBackoff, ReconnectPolicy};

use crate::watchdog::SmWatchdog;

/// Factory closure producing a fresh [`ReconnectPolicy`] instance per
/// recovery episode. Mirrors the shape used by
/// `taktora_connector_can::CanConnectorOptions`. `REQ_0332`.
pub type ReconnectPolicyFactory = Arc<dyn Fn() -> Box<dyn ReconnectPolicy> + Send + Sync + 'static>;

/// One SubDevice's PDO mapping. Application code declares an array of
/// these as a `static` and passes the slice to
/// [`EthercatConnectorOptionsBuilder::pdo_map`].
///
/// This struct is `#[non_exhaustive]`: out-of-crate code cannot build
/// it with a struct literal and must use [`SubDeviceMap::new`] plus the
/// chainable [`SubDeviceMap::with_sm_watchdog`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct SubDeviceMap {
    /// SubDevice configured address on the EtherCAT bus.
    pub address: u16,
    /// Mapped RxPDO entries (MainDevice → SubDevice flow).
    pub rx_pdos: &'static [PdoEntry],
    /// Mapped TxPDO entries (SubDevice → MainDevice flow).
    pub tx_pdos: &'static [PdoEntry],
    /// Expected working-counter contribution of this SubDevice on
    /// every healthy cycle. `REQ_0329`.
    ///
    /// EtherCAT's LRW datagram contributes +1 per SubDevice it
    /// writes to and +2 per SubDevice it reads from. The canonical
    /// values are 0 (no PDOs / coupler), 1 (RxPDOs only / outputs),
    /// 2 (TxPDOs only / inputs), 3 (both directions).
    pub expected_wkc: u16,
    /// SM-watchdog registers to program during bring-up and recovery,
    /// or `None` to leave the SubDevice's registers untouched.
    /// `REQ_0846`.
    ///
    /// `None` mirrors IgH's 0-sentinel and TwinCAT's unticked
    /// watchdog-config checkbox: the master writes nothing and the ESC
    /// keeps its power-up window. `Some(wd)` makes the gateway program
    /// `0x0400`/`0x0420`, read them back, and fail the attempt on
    /// mismatch — the enforcement path for safety assumption
    /// `AOU_0016`.
    pub sm_watchdog: Option<SmWatchdog>,
}

impl SubDeviceMap {
    /// Construct a map with no SM-watchdog programming
    /// (`sm_watchdog: None`).
    ///
    /// This is the only construction path for out-of-crate code: the
    /// struct is `#[non_exhaustive]`, so a literal will not compile
    /// outside this crate. Chain [`Self::with_sm_watchdog`] to enable
    /// watchdog enforcement.
    #[must_use]
    pub const fn new(
        address: u16,
        rx_pdos: &'static [PdoEntry],
        tx_pdos: &'static [PdoEntry],
        expected_wkc: u16,
    ) -> Self {
        Self {
            address,
            rx_pdos,
            tx_pdos,
            expected_wkc,
            sm_watchdog: None,
        }
    }

    /// Attach SM-watchdog registers to program during bring-up and
    /// recovery. `REQ_0846`.
    #[must_use]
    pub const fn with_sm_watchdog(mut self, wd: SmWatchdog) -> Self {
        self.sm_watchdog = Some(wd);
        self
    }
}

/// One mapped object within a PDO. `index` is the SDO index of the
/// mapped object; `bit_offset` and `bit_length` position it within the
/// PDO's process data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PdoEntry {
    /// SDO index of the mapped object.
    pub index: u16,
    /// Bit offset within the PDO process data.
    pub bit_offset: u16,
    /// Bit length of the mapped object.
    pub bit_length: u16,
}

/// Built `EthercatConnectorOptions`. Constructed via
/// [`EthercatConnectorOptionsBuilder`]; never mutated after build.
#[derive(Clone)]
pub struct EthercatConnectorOptions {
    cycle_time: Duration,
    distributed_clocks: bool,
    outbound_capacity: usize,
    inbound_capacity: usize,
    inbound_drop_threshold: u64,
    network_interface: Option<String>,
    pdo_map: &'static [SubDeviceMap],
    tokio_worker_threads: usize,
    reconnect_policy_factory: ReconnectPolicyFactory,
}

impl core::fmt::Debug for EthercatConnectorOptions {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("EthercatConnectorOptions")
            .field("cycle_time", &self.cycle_time)
            .field("distributed_clocks", &self.distributed_clocks)
            .field("outbound_capacity", &self.outbound_capacity)
            .field("inbound_capacity", &self.inbound_capacity)
            .field("inbound_drop_threshold", &self.inbound_drop_threshold)
            .field("network_interface", &self.network_interface)
            .field("pdo_map", &self.pdo_map)
            .field("tokio_worker_threads", &self.tokio_worker_threads)
            .field(
                "reconnect_policy_factory",
                &"<Fn() -> Box<dyn ReconnectPolicy>>",
            )
            .finish()
    }
}

impl EthercatConnectorOptions {
    /// Start a builder with default values per the spec.
    #[must_use]
    pub const fn builder() -> EthercatConnectorOptionsBuilder {
        EthercatConnectorOptionsBuilder::new()
    }

    /// Cycle duration (`REQ_0316`). Default 2 ms, minimum 1 ms.
    #[must_use]
    pub const fn cycle_time(&self) -> Duration {
        self.cycle_time
    }

    /// Distributed Clocks bring-up flag (`REQ_0318`). Default `false`.
    #[must_use]
    pub const fn distributed_clocks(&self) -> bool {
        self.distributed_clocks
    }

    /// Configured outbound bridge capacity (`REQ_0322`). Default 256.
    #[must_use]
    pub const fn outbound_capacity(&self) -> usize {
        self.outbound_capacity
    }

    /// Configured inbound bridge capacity (`REQ_0322`). Default 256.
    #[must_use]
    pub const fn inbound_capacity(&self) -> usize {
        self.inbound_capacity
    }

    /// Cumulative inbound-drop count that, once crossed, triggers a
    /// single `ConnectorHealth::Degraded { reason: "dropped N inbound frames" }`
    /// transition (`REQ_0324`). Default `1`. Emitted at most once per
    /// `Up → Degraded` cycle; the next stack-driven `→ Up` transition
    /// re-arms the latch.
    #[must_use]
    pub const fn inbound_drop_threshold(&self) -> u64 {
        self.inbound_drop_threshold
    }

    /// Network interface name the gateway will open (e.g. `"eth0"`).
    /// `None` selects the platform default.
    #[must_use]
    pub fn network_interface(&self) -> Option<&str> {
        self.network_interface.as_deref()
    }

    /// PDO mapping descriptor.
    #[must_use]
    pub const fn pdo_map(&self) -> &'static [SubDeviceMap] {
        self.pdo_map
    }

    /// Tokio worker-thread count for the gateway's sidecar
    /// (`ADR_0026`). Default 1.
    #[must_use]
    pub const fn tokio_worker_threads(&self) -> usize {
        self.tokio_worker_threads
    }

    /// Produce a fresh [`ReconnectPolicy`] for one recovery episode.
    /// `REQ_0332`.
    #[must_use]
    pub fn new_reconnect_policy(&self) -> Box<dyn ReconnectPolicy> {
        (self.reconnect_policy_factory)()
    }

    /// The configured factory itself. Useful when the gateway needs
    /// to share the factory with auxiliary tasks.
    #[must_use]
    pub fn reconnect_policy_factory(&self) -> ReconnectPolicyFactory {
        Arc::clone(&self.reconnect_policy_factory)
    }
}

/// Builder for [`EthercatConnectorOptions`].
#[derive(Clone)]
pub struct EthercatConnectorOptionsBuilder {
    cycle_time: Duration,
    distributed_clocks: bool,
    outbound_capacity: usize,
    inbound_capacity: usize,
    inbound_drop_threshold: u64,
    network_interface: Option<String>,
    pdo_map: &'static [SubDeviceMap],
    tokio_worker_threads: usize,
    reconnect_policy_factory: Option<ReconnectPolicyFactory>,
}

impl core::fmt::Debug for EthercatConnectorOptionsBuilder {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("EthercatConnectorOptionsBuilder")
            .field("cycle_time", &self.cycle_time)
            .field("distributed_clocks", &self.distributed_clocks)
            .field("outbound_capacity", &self.outbound_capacity)
            .field("inbound_capacity", &self.inbound_capacity)
            .field("inbound_drop_threshold", &self.inbound_drop_threshold)
            .field("network_interface", &self.network_interface)
            .field("pdo_map", &self.pdo_map)
            .field("tokio_worker_threads", &self.tokio_worker_threads)
            .field(
                "reconnect_policy_factory",
                &self
                    .reconnect_policy_factory
                    .as_ref()
                    .map(|_| "<Fn() -> Box<dyn ReconnectPolicy>>"),
            )
            .finish()
    }
}

const EMPTY_PDO_MAP: &[SubDeviceMap] = &[];

impl EthercatConnectorOptionsBuilder {
    /// Construct a builder with default values:
    ///
    /// * `cycle_time` — 2 ms (`REQ_0316`).
    /// * `distributed_clocks` — `false` (`REQ_0318`).
    /// * `outbound_capacity` / `inbound_capacity` — 256.
    /// * `inbound_drop_threshold` — 1 (`REQ_0324`).
    /// * `network_interface` — `None`.
    /// * `pdo_map` — empty slice; must be set to a non-empty value
    ///   for a useful gateway.
    /// * `tokio_worker_threads` — 1.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            cycle_time: Duration::from_millis(2),
            distributed_clocks: false,
            outbound_capacity: 256,
            inbound_capacity: 256,
            inbound_drop_threshold: 1,
            network_interface: None,
            pdo_map: EMPTY_PDO_MAP,
            tokio_worker_threads: 1,
            reconnect_policy_factory: None,
        }
    }

    /// Override the cycle duration. Values below 1 ms are clamped to
    /// 1 ms at [`Self::build`] time (`REQ_0316`).
    #[must_use]
    pub const fn cycle_time(mut self, d: Duration) -> Self {
        self.cycle_time = d;
        self
    }

    /// Enable or disable Distributed Clocks bring-up (`REQ_0318`).
    #[must_use]
    pub const fn distributed_clocks(mut self, on: bool) -> Self {
        self.distributed_clocks = on;
        self
    }

    /// Outbound bridge capacity (`REQ_0322`). Must be positive;
    /// zero is treated as 1 at build time.
    #[must_use]
    pub const fn outbound_capacity(mut self, n: usize) -> Self {
        self.outbound_capacity = n;
        self
    }

    /// Inbound bridge capacity (`REQ_0322`).
    #[must_use]
    pub const fn inbound_capacity(mut self, n: usize) -> Self {
        self.inbound_capacity = n;
        self
    }

    /// Cumulative inbound-drop threshold (`REQ_0324`). Values below 1
    /// are clamped to 1 at [`Self::build`] time.
    #[must_use]
    pub const fn inbound_drop_threshold(mut self, n: u64) -> Self {
        self.inbound_drop_threshold = n;
        self
    }

    /// Network interface name (e.g. `"eth0"`).
    #[must_use]
    pub fn network_interface(mut self, name: impl Into<String>) -> Self {
        self.network_interface = Some(name.into());
        self
    }

    /// PDO mapping descriptor.
    #[must_use]
    pub const fn pdo_map(mut self, map: &'static [SubDeviceMap]) -> Self {
        self.pdo_map = map;
        self
    }

    /// Tokio worker-thread count (`ADR_0026`). Values below 1 are
    /// clamped to 1 at [`Self::build`] time.
    #[must_use]
    pub const fn tokio_worker_threads(mut self, n: usize) -> Self {
        self.tokio_worker_threads = n;
        self
    }

    /// Override the default `ReconnectPolicy` factory. The closure is
    /// called once per recovery episode; the returned policy governs
    /// backoff and attempt budget for that episode.
    #[must_use]
    pub fn reconnect_policy_factory(mut self, factory: ReconnectPolicyFactory) -> Self {
        self.reconnect_policy_factory = Some(factory);
        self
    }

    /// Finalise. Applies the minimum-cycle-time clamp (1 ms) and
    /// capacity clamps (at least 1).
    #[must_use]
    pub fn build(self) -> EthercatConnectorOptions {
        let cycle_time = self.cycle_time.max(Duration::from_millis(1));
        let reconnect_policy_factory: ReconnectPolicyFactory = self
            .reconnect_policy_factory
            .unwrap_or_else(|| Arc::new(|| Box::new(ExponentialBackoff::default())));
        EthercatConnectorOptions {
            cycle_time,
            distributed_clocks: self.distributed_clocks,
            outbound_capacity: self.outbound_capacity.max(1),
            inbound_capacity: self.inbound_capacity.max(1),
            inbound_drop_threshold: self.inbound_drop_threshold.max(1),
            network_interface: self.network_interface,
            pdo_map: self.pdo_map,
            tokio_worker_threads: self.tokio_worker_threads.max(1),
            reconnect_policy_factory,
        }
    }
}

impl Default for EthercatConnectorOptionsBuilder {
    fn default() -> Self {
        Self::new()
    }
}
