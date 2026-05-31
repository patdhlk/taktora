//! Faithful in-memory IR of an `EtherCAT` ESI document.
//!
//! The IR captures structure without resolving it: every declared PDO is
//! preserved with its `Sm`/`Fixed`/`Mandatory` metadata, PDO assignment
//! alternatives are captured (not resolved), and no bit offsets are baked in.

use taktora_fieldbus_od_core::{DataType, DictEntry, Identity};

use crate::raw_xml::RawXml;

/// A parsed ESI file: a vendor plus one or more device descriptions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EsiFile {
    /// The vendor that owns the file.
    pub vendor: Vendor,
    /// Devices described by the ESI file, in document order.
    pub devices: Vec<EsiDevice>,
}

/// The vendor block of an ESI file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vendor {
    /// `EtherCAT` vendor id.
    pub id: u32,
    /// Vendor display name, when present.
    pub name: Option<String>,
}

/// A single `EtherCAT` device description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EsiDevice {
    /// Device identity (vendor / product / revision).
    pub identity: Identity,
    /// Product name (`<Type>` text / `<Name>`), when present.
    pub name: Option<String>,
    /// Raw `<Type>` product-type string, when present.
    pub product_type: Option<String>,
    /// Device group type, when present.
    pub group_type: Option<String>,
    /// Sync managers in declaration order.
    pub sync_managers: Vec<SyncManager>,
    /// `TxPDOs` (`SubDevice` → master), each preserved structurally.
    pub tx_pdos: Vec<Pdo>,
    /// `RxPDOs` (master → `SubDevice`), each preserved structurally.
    pub rx_pdos: Vec<Pdo>,
    /// Mailbox configuration, when the device declares a mailbox.
    pub mailbox: Option<Mailbox>,
    /// Distributed-clock configuration, when present.
    pub dc: Option<DistributedClock>,
    /// Object-dictionary entries, when present.
    pub dictionary: Vec<DictEntry>,
    /// Unrecognised device-level vendor extension elements, captured verbatim.
    pub vendor_extensions: Vec<RawXml>,
}

/// Direction a sync manager carries data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmDirection {
    /// Master → `SubDevice` (outputs).
    Output,
    /// `SubDevice` → master (inputs).
    Input,
    /// Mailbox or unspecified.
    Unspecified,
}

/// One sync manager.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncManager {
    /// Sync manager index (0-based).
    pub index: u8,
    /// Physical start address.
    pub start_address: u16,
    /// Control byte.
    pub control_byte: u8,
    /// Whether the sync manager is enabled.
    pub enable: bool,
    /// Direction, derived from the control byte / `<Sm>` attributes.
    pub direction: SmDirection,
}

/// One PDO (a `<TxPdo>` or `<RxPdo>` element), preserved structurally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pdo {
    /// PDO mapping object index (e.g. `0x1A00`).
    pub index: u16,
    /// PDO name, when present.
    pub name: Option<String>,
    /// Sync manager this PDO is assigned to (`Sm` attribute), when present.
    pub sm: Option<u8>,
    /// `Fixed` attribute — the mapping is not reconfigurable.
    pub fixed: bool,
    /// `Mandatory` attribute — the PDO is always active.
    pub mandatory: bool,
    /// Indices of PDOs excluded by this one (`<Exclude>` children).
    pub exclude: Vec<u16>,
    /// Entries of this PDO, in declaration order (including padding entries).
    pub entries: Vec<PdoEntry>,
}

/// One PDO entry. A padding entry has `index == 0`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdoEntry {
    /// Object-dictionary index the entry maps (`0` = padding/gap).
    pub index: u16,
    /// Sub-index the entry maps.
    pub sub_index: u8,
    /// Length of the entry in bits.
    pub bit_length: u16,
    /// Entry name, when present.
    pub name: Option<String>,
    /// Entry data type, when present.
    pub data_type: Option<DataType>,
}

/// Mailbox configuration.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Mailbox {
    /// `CoE` support details, when `CoE` is declared.
    pub coe: Option<CoeInfo>,
    /// `EoE` (Ethernet over `EtherCAT`) supported.
    pub eoe: bool,
    /// File-over-EtherCAT mailbox protocol supported.
    pub foe: bool,
    /// `SoE` (Servo over `EtherCAT`) supported.
    pub soe: bool,
    /// `VoE` (Vendor over `EtherCAT`) supported.
    pub voe: bool,
    /// Init commands (SDO writes) by transition.
    pub init_cmds: Vec<InitCmd>,
}

/// `CoE` mailbox capability flags.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CoeInfo {
    /// SDO info service supported.
    pub sdo_info: bool,
    /// PDO assignment (0x1C12/0x1C13) configurable.
    pub pdo_assign: bool,
    /// PDO configuration configurable.
    pub pdo_config: bool,
    /// Complete-access supported.
    pub complete_access: bool,
}

/// State-machine transition an init command runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    /// Init → Pre-Op.
    Ip,
    /// Pre-Op → Safe-Op.
    Ps,
    /// Safe-Op → Op.
    So,
    /// Another / unrecognised transition.
    Other,
}

/// One init command: an SDO write bound to a transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitCmd {
    /// Transition the command runs on.
    pub transition: Transition,
    /// Object index written.
    pub index: u16,
    /// Sub-index written.
    pub sub_index: u8,
    /// Raw payload bytes.
    pub data: Vec<u8>,
    /// Human-readable comment, when present.
    pub comment: Option<String>,
}

/// Distributed-clock configuration.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DistributedClock {
    /// Declared operation modes.
    pub op_modes: Vec<DcOpMode>,
}

/// One distributed-clock operation mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DcOpMode {
    /// Mode name.
    pub name: String,
    /// Mode description, when present.
    pub desc: Option<String>,
    /// `AssignActivate` register value.
    pub assign_activate: u16,
    /// SYNC0 cycle time (ns), when present.
    pub cycle_time_sync0: Option<i32>,
    /// SYNC0 shift time (ns), when present.
    pub shift_time_sync0: Option<i32>,
    /// SYNC1 cycle time (ns), when present.
    pub cycle_time_sync1: Option<i32>,
    /// SYNC1 shift time (ns), when present.
    pub shift_time_sync1: Option<i32>,
}
