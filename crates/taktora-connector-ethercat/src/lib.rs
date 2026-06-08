//! EtherCAT reference connector — `BB_0030` / `FEAT_0041`.
//!
//! This crate is being delivered in stages. The current commit (C5a)
//! lands the protocol-agnostic core:
//!
//! * [`routing::EthercatRouting`] (`REQ_0311`).
//! * [`options::EthercatConnectorOptions`] typed builder + the
//!   `&'static [SubDeviceMap]` PDO descriptor shape (`REQ_0314`,
//!   `REQ_0316`, `REQ_0322`, `ADR_0027`).
//! * [`bridge::OutboundBridge`] / [`bridge::InboundBridge`] with
//!   `BackPressure` and `DroppedInbound` semantics (`REQ_0322`–
//!   `REQ_0324`).
//! * [`health::EthercatHealthMonitor`] wrapping
//!   `taktora_connector_core::HealthMonitor` and broadcasting
//!   [`HealthEvent`]s through a `crossbeam_channel`.
//! * [`gateway::EthercatGateway`] — owns a tokio runtime that is
//!   joined on `Drop` with a 5-second budget (`ADR_0026`,
//!   `REQ_0321`).
//! * [`connector::EthercatConnector`] — implements
//!   [`taktora_connector_host::Connector`] (`REQ_0310`).
//!
//! `ethercrab` integration is in place — [`EthercrabBusDriver`]
//! (gated on the `bus-integration` cargo feature) wraps
//! `ethercrab::MainDevice`, drives the bus through PRE-OP → SAFE-OP
//! → OP, applies per-SubDevice SDO PDO mapping, and supports
//! bus-level recovery via [`BusDriver::recover`] driven by
//! [`EthercatConnectorOptions::reconnect_policy_factory`].
//! Distributed Clocks opt-in currently applies during bring-up
//! only via `MainDeviceConfig::dc_static_sync_iterations`; the
//! per-cycle `tx_rx_dc` branch (`REQ_0330`) is a follow-on. See
//! [`crate::bus`] for the production-driver overview and
//! [`MockBusDriver`] for the in-tree test substitute. `REQ_0312` /
//! `REQ_0313` / `REQ_0314` / `REQ_0315` / `REQ_0329` / `REQ_0331` /
//! `REQ_0332`.
//!
//! [`HealthEvent`]: taktora_connector_core::HealthEvent

#![warn(missing_docs)]
// Allow EtherCAT domain identifiers (SubDevice, MainDevice, RxPdo /
// TxPdo, CAP_NET_RAW, etc.) to appear in docstrings without backticks.
// The framework's other crates accept this lint, but EtherCAT
// terminology repeats too often inside our own doc comments to be
// worth backticking individually.
#![allow(clippy::doc_markdown)]

pub mod bridge;
#[cfg(feature = "bus-integration")]
pub mod bus;
pub mod connector;
pub mod dispatcher;
pub mod driver;
#[cfg(feature = "bus-integration")]
pub mod ethercrab_driver;
pub mod gateway;
pub mod health;
pub mod mock;
pub mod op_transition;
pub mod options;
pub mod pdi;
pub mod registry;
pub mod routing;
pub mod runner;
pub mod scheduler;
pub mod sdo;
pub mod watchdog;
pub mod wkc;

pub use bridge::{InboundBridge, InboundOutcome, OutboundBridge, OutboundError};
pub use connector::EthercatConnector;
pub use dispatcher::{
    BridgedInboundPublish, DispatchReport, IoxInboundPublish, IoxOutboundDrain, dispatch_one_cycle,
    dispatcher_loop,
};
pub use driver::{BringUp, BusDriver};
#[cfg(feature = "bus-integration")]
pub use ethercrab_driver::EthercrabBusDriver;
pub use gateway::EthercatGateway;
pub use health::EthercatHealthMonitor;
pub use mock::{CycleKind, MockBusDriver};
pub use options::{
    EthercatConnectorOptions, EthercatConnectorOptionsBuilder, PdoEntry, ReconnectPolicyFactory,
    StartupSdo, SubDeviceMap,
};
pub use registry::{
    ChannelBinding, ChannelHandle, ChannelRegistry, InboundPublish, OutboundDrain,
    RegisteredChannel,
};
pub use routing::{EthercatRouting, PdoDirection};
pub use runner::{CycleReport, CycleRunner};
pub use scheduler::{CycleDecision, CycleScheduler};
pub use sdo::{
    SM_ASSIGN_RX_PDO, SM_ASSIGN_TX_PDO, SdoValue, SdoWrite, pdo_sdo_writes, startup_sdo_writes,
};
pub use watchdog::SmWatchdog;
pub use wkc::{WkcVerdict, evaluate_wkc, expected_wkc_from_map};
