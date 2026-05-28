//! Forward-compatible declarations for ethercrab integration. Gated
//! on the `bus-integration` cargo feature.
//!
//! This module provides:
//!
//! * [`EthercatPduStorage`] — the default [`PduStorage`] type alias
//!   carrying ethercrab's recommended frame pool size.
//! * [`crate::declare_pdu_storage`] — a macro that declares a
//!   `static` of [`EthercatPduStorage`] in application code, ready
//!   to pass into the production [`crate::EthercrabBusDriver`].
//!
//! ## Production driver
//!
//! [`crate::EthercrabBusDriver`] wraps `ethercrab::MainDevice`,
//! spawns `ethercrab::std::tx_rx_task` on a tokio runtime, and
//! drives the bus through PRE-OP → SAFE-OP → OP per `REQ_0312` /
//! `REQ_0313` / `REQ_0315`. It implements [`crate::BusDriver`]:
//!
//! * `bring_up` — splits storage, constructs `MainDevice`, spawns
//!   `tx_rx_task`, calls `init_single_group`, applies the
//!   per-SubDevice SDO writes from [`crate::pdo_sdo_writes`], walks
//!   into OP, returns [`crate::BringUp`] with the asymmetric WKC
//!   computed from each [`crate::SubDeviceMap`]'s `expected_wkc`
//!   (`REQ_0329`).
//! * `cycle` — calls `group.tx_rx` per cycle (`REQ_0312`).
//!   Distributed Clocks opt-in
//!   ([`crate::EthercatConnectorOptions::distributed_clocks`])
//!   currently applies during bring-up only via
//!   `MainDeviceConfig::dc_static_sync_iterations`; the per-cycle
//!   `group.tx_rx_dc` branch (`REQ_0330`) is tracked as a follow-on.
//! * `recover` — drops the in-flight `SubDeviceGroup`, re-runs
//!   `init_single_group` against the same `MainDevice`, re-applies
//!   the PDO mapping, walks back into OP (`REQ_0331`). The
//!   `MainDevice` and `tx_rx_task` are preserved across the call —
//!   [`PduStorage::try_split`] is one-shot and cannot be re-run.
//!
//! ## Recovery scope
//!
//! Bus-level faults (SubDevice drops, persistent WKC mismatch,
//! OP→SAFE-OP fallback) are recoverable via `recover`, driven from
//! the cycle runner per a configurable
//! [`crate::EthercatConnectorOptions::reconnect_policy_factory`]
//! (`REQ_0332`). NIC-level failure (the `tx_rx_task` future itself
//! returning `Err`) is **terminal**: the cycle runner emits a
//! terminal `Down` and exits. Re-spawning `tx_rx_task` would require
//! splitting a fresh `PduStorage`, which ethercrab does not support.
//!
//! ## Tests
//!
//! * `crates/taktora-connector-ethercat/tests/ethercrab_driver.rs`
//!   — compile-checked + `#[ignore]`-gated against ethercrab 0.7;
//!   set `ETHERCAT_TEST_NIC` to run the hardware-side tests including
//!   `recover_returns_to_op_without_storage_resplit`.
//! * [`crate::MockBusDriver`] — the in-tree test substitute. Used
//!   by `crates/taktora-connector-ethercat-tests/tests/recovery.rs`
//!   for the recovery state machine + asymmetric WKC + CycleKind
//!   recorder.

use ethercrab::PduStorage;

/// PDU pool frame count used by the default storage type
/// ([`EthercatPduStorage`]). Matches ethercrab's recommended size for
/// modest bus topologies.
pub const ETHERCAT_MAX_FRAMES: usize = 16;

/// Maximum single-frame PDU payload size used by the default storage
/// type ([`EthercatPduStorage`]). Per ethercrab convention.
pub const ETHERCAT_MAX_PDU_DATA: usize = 1100;

/// Default [`PduStorage`] type for taktora-connector-ethercat.
///
/// Declare a `static` of this type via
/// [`crate::declare_pdu_storage!`] and pass a reference into the
/// bring-up flow via [`crate::EthercrabBusDriver`]. Each storage can
/// produce one `MainDevice` ([`PduStorage::try_split`] is one-shot),
/// so applications wanting multiple gateways declare one storage per
/// gateway (`REQ_0312`).
pub type EthercatPduStorage =
    PduStorage<ETHERCAT_MAX_FRAMES, { PduStorage::element_size(ETHERCAT_MAX_PDU_DATA) }>;

/// Declare a `static` [`EthercatPduStorage`] with the default frame
/// pool size. Application code calls this once per planned gateway:
///
/// ```ignore
/// taktora_connector_ethercat::declare_pdu_storage!(BUS_STORAGE);
/// ```
///
/// then later passes `&BUS_STORAGE` to the bring-up flow.
#[macro_export]
macro_rules! declare_pdu_storage {
    ($name:ident) => {
        static $name: $crate::bus::EthercatPduStorage = $crate::bus::EthercatPduStorage::new();
    };
}
