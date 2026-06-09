//! Private serde-derive DTOs mirroring the ESI XML shape, plus conversion to
//! the public IR. Integral ESI fields arrive as `String` (because of the `#x`
//! hex form) and are converted via [`parse_esi_uint`].

use serde::Deserialize;
use taktora_fieldbus_od_core::{DataType, Identity};

use crate::error::EsiError;
use crate::model::{
    AltPdoRef, AlternativeSmMapping, CoeInfo, DcOpMode, DistributedClock, EsiDevice, EsiFile,
    InitCmd, Mailbox, Module, Pdo, PdoEntry, Slot, SlotModuleIdent, Slots, SmAssignment,
    Transition, Vendor,
};

// ── integer / bool helpers ────────────────────────────────────────────────────

/// Parse an ESI integer: `#x`/`#X`-prefixed hex, or plain decimal.
pub fn parse_esi_uint(raw: &str, path: &str) -> Result<u32, EsiError> {
    let t = raw.trim();
    let parsed = t
        .strip_prefix("#x")
        .or_else(|| t.strip_prefix("#X"))
        .map_or_else(
            || t.parse::<u32>().ok(),
            |hex| u32::from_str_radix(hex, 16).ok(),
        );
    parsed.ok_or_else(|| EsiError::Number {
        raw: t.to_owned(),
        path: path.to_owned(),
    })
}

fn parse_esi_u16(raw: &str, path: &str) -> Result<u16, EsiError> {
    u16::try_from(parse_esi_uint(raw, path)?).map_err(|_| EsiError::Number {
        raw: raw.trim().to_owned(),
        path: path.to_owned(),
    })
}

fn parse_esi_u8(raw: &str, path: &str) -> Result<u8, EsiError> {
    u8::try_from(parse_esi_uint(raw, path)?).map_err(|_| EsiError::Number {
        raw: raw.trim().to_owned(),
        path: path.to_owned(),
    })
}

fn parse_esi_bool(raw: Option<&String>) -> bool {
    matches!(raw.map(|s| s.trim()), Some("1" | "true" | "TRUE"))
}

// ── localized name helpers ────────────────────────────────────────────────────

/// A `<Name>` element, possibly carrying a locale id. Real Beckhoff ESI repeats
/// `<Name LcId="1033">` (English) / `<Name LcId="1031">` (German); CDATA content
/// is captured by quick-xml's `$text`.
#[derive(Deserialize)]
struct NameDto {
    #[serde(rename = "@LcId", default)]
    lc_id: Option<u32>,
    #[serde(rename = "$text", default)]
    text: Option<String>,
}

/// English (`LcId == 1033`) Windows locale id.
const LCID_ENGLISH: u32 = 1033;

/// Pick a single display name from a list of localized `<Name>` elements.
///
/// Prefers the English (`LcId == 1033`) entry; otherwise the first non-empty
/// text in document order. Returns `None` if no entry carries text.
fn pick_name(names: &[NameDto]) -> Option<String> {
    let non_empty = |t: &Option<String>| t.as_deref().is_some_and(|s| !s.trim().is_empty());
    names
        .iter()
        .find(|n| n.lc_id == Some(LCID_ENGLISH) && non_empty(&n.text))
        .or_else(|| names.iter().find(|n| non_empty(&n.text)))
        .and_then(|n| n.text.clone())
}

// ── top-level DTOs ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct EtherCatInfo {
    #[serde(rename = "Vendor")]
    vendor: VendorDto,
    #[serde(rename = "Descriptions")]
    descriptions: Descriptions,
}

#[derive(Deserialize)]
struct VendorDto {
    #[serde(rename = "Id")]
    id: String,
    #[serde(rename = "Name", default)]
    names: Vec<NameDto>,
}

#[derive(Deserialize)]
struct Descriptions {
    #[serde(rename = "Devices", default)]
    devices: Option<Devices>,
    #[serde(rename = "Modules", default)]
    modules: Option<ModulesDto>,
}

#[derive(Deserialize)]
struct Devices {
    #[serde(rename = "Device", default)]
    device: Vec<DeviceDto>,
}

#[derive(Deserialize)]
struct DeviceDto {
    #[serde(rename = "Type")]
    ty: TypeDto,
    #[serde(rename = "Name", default)]
    names: Vec<NameDto>,
    #[serde(rename = "GroupType", default)]
    group_type: Option<String>,
    #[serde(rename = "Fmmu", default)]
    fmmu: Vec<FmmuDto>,
    #[serde(rename = "Sm", default)]
    sm: Vec<SmDto>,
    #[serde(rename = "TxPdo", default)]
    tx_pdo: Vec<PdoDto>,
    #[serde(rename = "RxPdo", default)]
    rx_pdo: Vec<PdoDto>,
    #[serde(rename = "Mailbox", default)]
    mailbox: Option<MailboxDto>,
    #[serde(rename = "Profile", default)]
    profile: Option<ProfileDto>,
    #[serde(rename = "Dc", default)]
    dc: Option<DcDto>,
    #[serde(rename = "Eeprom", default)]
    eeprom: Option<EepromDto>,
    #[serde(rename = "Slots", default)]
    slots: Option<SlotsDto>,
    #[serde(rename = "Info", default)]
    info: Option<DeviceInfoDto>,
}

#[derive(Deserialize)]
struct TypeDto {
    // Some real files carry placeholder `<Type>` elements with neither a
    // product code nor a revision (e.g. abstract module slots); default both
    // to 0 rather than reject the whole document.
    //
    // TODO(FEAT_0051 follow-up): a <Type> with no ProductCode/RevisionNo is a
    // placeholder/abstract slot, not a real addressable device. Defaulting
    // identity to 0 here is provisional tolerance; codegen keys on identity, so
    // the cleaner long-term behaviour is to SKIP such devices rather than emit a
    // zero-identity (which would collide across placeholders). Tracked for the
    // real-device codegen slices.
    #[serde(rename = "@ProductCode", default)]
    product_code: Option<String>,
    #[serde(rename = "@RevisionNo", default)]
    revision_no: Option<String>,
    #[serde(rename = "$text", default)]
    text: Option<String>,
}

// ── device-level Info / AlternativeSmMapping DTOs ─────────────────────────────

/// Device-level `<Info>` element. Only the `<VendorSpecific>` child is read;
/// everything else (Electrical, Mailbox timeouts, …) is tolerated and ignored.
#[derive(Deserialize)]
struct DeviceInfoDto {
    #[serde(rename = "VendorSpecific", default)]
    vendor_specific: Option<VendorSpecificDto>,
}

#[derive(Deserialize)]
struct VendorSpecificDto {
    #[serde(rename = "TwinCAT", default)]
    twin_cat: Option<TwinCatDto>,
}

#[derive(Deserialize)]
struct TwinCatDto {
    #[serde(rename = "AlternativeSmMapping", default)]
    alternative_sm_mapping: Vec<AltSmMappingDto>,
}

#[derive(Deserialize)]
struct AltSmMappingDto {
    #[serde(rename = "@Default", default)]
    default: Option<String>,
    // <AlternativeSmMapping><Name> is single-locale in Beckhoff ESI; Option<String> is deliberate (not Vec<NameDto>).
    #[serde(rename = "Name", default)]
    name: Option<String>,
    #[serde(rename = "Sm", default)]
    sm: Vec<AltSmDto>,
}

#[derive(Deserialize)]
struct AltSmDto {
    #[serde(rename = "@No")]
    no: String,
    #[serde(rename = "Pdo", default)]
    pdo: Vec<AltPdoDto>,
}

#[derive(Deserialize)]
struct AltPdoDto {
    #[serde(rename = "@ChannelNo", default)]
    channel_no: Option<String>,
    #[serde(rename = "$text")]
    index: String,
}

// ── FMMU DTO ─────────────────────────────────────────────────────────────────

/// An `<Fmmu>` element. Real ESI carries attributes on this element
/// (e.g. `<Fmmu OpOnly="1">Outputs</Fmmu>` in Beckhoff EL2004); only the text
/// content ("Outputs") is meaningful here, so the attributes are tolerated and
/// ignored.
#[derive(Deserialize)]
struct FmmuDto {
    #[serde(rename = "$text", default)]
    text: Option<String>,
}

// ── sync manager DTO ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct SmDto {
    // Real Beckhoff files carry placeholder `<Sm No="0">` / `<Sm … Virtual="true">`
    // declarations with neither a start address nor a control byte; default both
    // to 0 so the device still parses (such an Sm is simply unconfigured).
    #[serde(rename = "@StartAddress", default)]
    start_address: Option<String>,
    #[serde(rename = "@ControlByte", default)]
    control_byte: Option<String>,
    #[serde(rename = "@Enable", default)]
    enable: Option<String>,
}

// ── PDO DTOs ──────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct PdoDto {
    #[serde(rename = "@Sm", default)]
    sm: Option<String>,
    #[serde(rename = "@Fixed", default)]
    fixed: Option<String>,
    #[serde(rename = "@Mandatory", default)]
    mandatory: Option<String>,
    #[serde(rename = "Index")]
    index: String,
    #[serde(rename = "Name", default)]
    name: Option<String>,
    #[serde(rename = "Exclude", default)]
    exclude: Vec<String>,
    #[serde(rename = "Entry", default)]
    entry: Vec<EntryDto>,
}

#[derive(Deserialize)]
struct EntryDto {
    #[serde(rename = "Index")]
    index: String,
    #[serde(rename = "SubIndex", default)]
    sub_index: Option<String>,
    #[serde(rename = "BitLen")]
    bit_len: String,
    #[serde(rename = "Name", default)]
    name: Option<String>,
    #[serde(rename = "DataType", default)]
    data_type: Option<DataTypeDto>,
}

/// An entry's `<DataType>` element. Real ESI carries attributes on this element
/// (e.g. `<DataType DScale="+/-10">DINT</DataType>`); only the text content
/// ("DINT") is meaningful here, so the attributes are tolerated and ignored.
#[derive(Deserialize)]
struct DataTypeDto {
    #[serde(rename = "$text", default)]
    text: Option<String>,
}

// ── mailbox DTOs ──────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct MailboxDto {
    #[serde(rename = "CoE", default)]
    coe: Option<CoeDto>,
    #[serde(rename = "EoE", default)]
    eoe: Option<Empty>,
    #[serde(rename = "FoE", default)]
    foe: Option<Empty>,
    #[serde(rename = "SoE", default)]
    soe: Option<Empty>,
    #[serde(rename = "VoE", default)]
    voe: Option<Empty>,
}

#[derive(Deserialize)]
struct Empty {}

#[derive(Deserialize)]
struct CoeDto {
    #[serde(rename = "@SdoInfo", default)]
    sdo_info: Option<String>,
    #[serde(rename = "@PdoAssign", default)]
    pdo_assign: Option<String>,
    #[serde(rename = "@PdoConfig", default)]
    pdo_config: Option<String>,
    #[serde(rename = "@CompleteAccess", default)]
    complete_access: Option<String>,
}

// ── profile / dictionary / init-cmds DTOs ────────────────────────────────────

#[derive(Deserialize)]
struct ProfileDto {
    #[serde(rename = "Dictionary", default)]
    dictionary: Option<DictionaryDto>,
}

#[derive(Deserialize)]
struct DictionaryDto {
    #[serde(rename = "InitCmds", default)]
    init_cmds: Option<InitCmdsDto>,
    #[serde(rename = "Objects", default)]
    objects: Option<ObjectsDto>,
}

#[derive(Deserialize)]
struct InitCmdsDto {
    #[serde(rename = "InitCmd", default)]
    init_cmd: Vec<InitCmdDto>,
}

#[derive(Deserialize)]
struct InitCmdDto {
    #[serde(rename = "Transition", default)]
    transition: Vec<String>,
    #[serde(rename = "Index")]
    index: String,
    #[serde(rename = "SubIndex", default)]
    sub_index: Option<String>,
    #[serde(rename = "Data", default)]
    data: Option<String>,
    #[serde(rename = "Comment", default)]
    comment: Option<String>,
}

// ── object dictionary DTOs ────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ObjectsDto {
    #[serde(rename = "Object", default)]
    object: Vec<ObjectDto>,
}

#[derive(Deserialize)]
struct ObjectDto {
    #[serde(rename = "Index")]
    index: String,
    #[serde(rename = "Name", default)]
    name: Option<String>,
    #[serde(rename = "Type", default)]
    ty: Option<String>,
    #[serde(rename = "BitSize", default)]
    bit_size: Option<String>,
    #[serde(rename = "Info", default)]
    info: Option<InfoDto>,
    #[serde(rename = "Flags", default)]
    flags: Option<FlagsDto>,
    #[serde(rename = "SubItem", default)]
    sub_item: Vec<SubItemDto>,
}

#[derive(Deserialize)]
struct SubItemDto {
    #[serde(rename = "SubIndex", default)]
    sub_index: Option<String>,
    #[serde(rename = "Name", default)]
    name: Option<String>,
    #[serde(rename = "Type", default)]
    ty: Option<String>,
    #[serde(rename = "BitSize", default)]
    bit_size: Option<String>,
    #[serde(rename = "Info", default)]
    info: Option<InfoDto>,
    #[serde(rename = "Flags", default)]
    flags: Option<FlagsDto>,
}

#[derive(Deserialize)]
struct InfoDto {
    #[serde(rename = "DefaultData", default)]
    default_data: Option<String>,
}

#[derive(Deserialize)]
struct FlagsDto {
    #[serde(rename = "Access", default)]
    access: Option<String>,
    #[serde(rename = "PdoMapping", default)]
    pdo_mapping: Option<String>,
}

// ── eeprom DTO ────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct EepromDto {
    #[serde(rename = "ByteSize", default)]
    byte_size: Option<String>,
    #[serde(rename = "ConfigData", default)]
    config_data: Option<String>,
    #[serde(rename = "BootStrap", default)]
    boot_strap: Option<String>,
}

// ── MDP module / slot DTOs ────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ModulesDto {
    #[serde(rename = "Module", default)]
    module: Vec<ModuleDto>,
}

#[derive(Deserialize)]
struct ModuleDto {
    #[serde(rename = "Type")]
    ty: ModuleTypeDto,
    #[serde(rename = "Name", default)]
    names: Vec<NameDto>,
    #[serde(rename = "TxPdo", default)]
    tx_pdo: Vec<PdoDto>,
    #[serde(rename = "RxPdo", default)]
    rx_pdo: Vec<PdoDto>,
}

#[derive(Deserialize)]
struct ModuleTypeDto {
    #[serde(rename = "@ModuleIdent", default)]
    module_ident: Option<String>,
    #[serde(rename = "$text", default)]
    text: Option<String>,
}

#[derive(Deserialize)]
struct SlotsDto {
    #[serde(rename = "@SlotPdoIncrement", default)]
    slot_pdo_increment: Option<String>,
    #[serde(rename = "@SlotIndexIncrement", default)]
    slot_index_increment: Option<String>,
    #[serde(rename = "Slot", default)]
    slot: Vec<SlotDto>,
}

#[derive(Deserialize)]
struct SlotDto {
    #[serde(rename = "@MinInstances", default)]
    min_instances: Option<String>,
    #[serde(rename = "@MaxInstances", default)]
    max_instances: Option<String>,
    #[serde(rename = "Name", default)]
    names: Vec<NameDto>,
    #[serde(rename = "ModuleIdent", default)]
    module_ident: Vec<ModuleIdentDto>,
}

#[derive(Deserialize)]
struct ModuleIdentDto {
    #[serde(rename = "@Default", default)]
    default: Option<String>,
    #[serde(rename = "$text", default)]
    text: Option<String>,
}

// ── distributed clock DTOs ────────────────────────────────────────────────────

#[derive(Deserialize)]
struct DcDto {
    #[serde(rename = "OpMode", default)]
    op_mode: Vec<OpModeDto>,
}

#[derive(Deserialize)]
struct OpModeDto {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Desc", default)]
    desc: Option<String>,
    #[serde(rename = "AssignActivate")]
    assign_activate: String,
    #[serde(rename = "CycleTimeSync0", default)]
    cycle_time_sync0: Option<CycleTimeDto>,
    #[serde(rename = "ShiftTimeSync0", default)]
    shift_time_sync0: Option<String>,
    #[serde(rename = "CycleTimeSync1", default)]
    cycle_time_sync1: Option<CycleTimeDto>,
    #[serde(rename = "ShiftTimeSync1", default)]
    shift_time_sync1: Option<String>,
}

#[derive(Deserialize)]
struct CycleTimeDto {
    #[serde(rename = "$text", default)]
    value: Option<String>,
}

// ── sync manager conversion ───────────────────────────────────────────────────

/// Convert a vec of [`SmDto`]s into [`SyncManager`]s, assigning 0-based
/// indices from declaration order.
///
/// Direction is derived from control-byte bits 2..3
/// (`(control_byte >> 2) & 0x3`):
/// - `0b01` → [`SmDirection::Output`] (master writes the SM, i.e. master → `SubDevice`)
/// - `0b00` → [`SmDirection::Input`] (master reads the SM, i.e. `SubDevice` → master)
/// - anything else → [`SmDirection::Unspecified`]
///
/// `watchdog_trigger_enable` is derived from control-byte bit 6 (`0x40`), the
/// ETG SM-control watchdog-trigger-enable bit; an absent `ControlByte` decodes
/// to `false`.
fn sync_managers_from_dtos(dtos: Vec<SmDto>) -> Result<Vec<crate::model::SyncManager>, EsiError> {
    use crate::model::{SmDirection, SyncManager};
    let mut out = Vec::with_capacity(dtos.len());
    for (i, sm) in dtos.into_iter().enumerate() {
        let control_byte = match sm.control_byte.as_deref() {
            Some(cb) => parse_esi_u8(cb, "Sm.ControlByte")?,
            None => 0,
        };
        let direction = match (control_byte >> 2) & 0x3 {
            0b01 => SmDirection::Output,
            0b00 => SmDirection::Input,
            _ => SmDirection::Unspecified,
        };
        // Control-byte bit 6 (0x40) is the ETG SM-control
        // watchdog-trigger-enable bit. An absent `ControlByte` decoded to 0
        // above, so a missing attribute yields `false` — never a fabricated
        // enabled watchdog.
        let watchdog_trigger_enable = control_byte & 0x40 != 0;
        let start_address = match sm.start_address.as_deref() {
            Some(sa) => parse_esi_u16(sa, "Sm.StartAddress")?,
            None => 0,
        };
        out.push(SyncManager {
            index: u8::try_from(i).map_err(|_| EsiError::Number {
                raw: i.to_string(),
                path: "Sm.index".to_owned(),
            })?,
            start_address,
            control_byte,
            enable: parse_esi_bool(sm.enable.as_ref()),
            direction,
            watchdog_trigger_enable,
        });
    }
    Ok(out)
}

// ── FMMU conversion ───────────────────────────────────────────────────────────

/// Convert `<Fmmu>` text into [`FmmuUsage`]. Unknown strings are preserved as
/// [`FmmuUsage::Other`] — the parser never hard-fails on an unrecognised usage.
fn fmmus_from_dtos(dtos: Vec<FmmuDto>) -> Vec<crate::model::Fmmu> {
    use crate::model::{Fmmu, FmmuUsage};
    dtos.into_iter()
        .map(|f| {
            let text = f.text.as_deref().map(str::trim).unwrap_or_default();
            let usage = match text {
                "Inputs" => FmmuUsage::Inputs,
                "Outputs" => FmmuUsage::Outputs,
                "MBoxState" => FmmuUsage::MBoxState,
                other => FmmuUsage::Other(other.to_owned()),
            };
            Fmmu { usage }
        })
        .collect()
}

// ── PDO conversion ────────────────────────────────────────────────────────────

fn pdo_from_dto(dto: PdoDto) -> Result<Pdo, EsiError> {
    let index = parse_esi_u16(&dto.index, "Pdo.Index")?;
    let sm = dto
        .sm
        .as_deref()
        .map(|s| parse_esi_u8(s, "Pdo.Sm"))
        .transpose()?;

    let mut exclude = Vec::with_capacity(dto.exclude.len());
    for e in &dto.exclude {
        exclude.push(parse_esi_u16(e, "Pdo.Exclude")?);
    }

    let mut entries = Vec::with_capacity(dto.entry.len());
    for e in dto.entry {
        let sub_index = e
            .sub_index
            .as_deref()
            .map(|s| parse_esi_u8(s, "Entry.SubIndex"))
            .transpose()?
            .unwrap_or(0);
        entries.push(PdoEntry {
            index: parse_esi_u16(&e.index, "Entry.Index")?,
            sub_index,
            bit_length: parse_esi_u16(&e.bit_len, "Entry.BitLen")?,
            name: e.name,
            data_type: e
                .data_type
                .and_then(|d| d.text)
                .as_deref()
                .map(DataType::parse_coe_name),
        });
    }

    Ok(Pdo {
        index,
        name: dto.name,
        sm,
        fixed: parse_esi_bool(dto.fixed.as_ref()),
        mandatory: parse_esi_bool(dto.mandatory.as_ref()),
        exclude,
        entries,
    })
}

// ── mailbox conversion ────────────────────────────────────────────────────────

fn parse_hex_payload(s: &str) -> Vec<u8> {
    let cleaned: Vec<u8> = s.bytes().filter(u8::is_ascii_hexdigit).collect();
    cleaned
        .chunks(2)
        .filter_map(|pair| {
            let hi = char::from(pair[0]).to_digit(16)?;
            let lo = pair
                .get(1)
                .map_or(Some(0), |b| char::from(*b).to_digit(16))?;
            u8::try_from((hi << 4) | lo).ok()
        })
        .collect()
}

/// Parse a strict hex-byte string (`"0401AB"`) into bytes. Unlike
/// [`parse_hex_payload`] (lenient, for init-cmd payloads), a non-hex digit or
/// odd digit count is a semantic error carrying the element path.
fn parse_hex_bytes(raw: &str, path: &str) -> Result<Vec<u8>, EsiError> {
    let t = raw.trim();
    if t.len() % 2 != 0 || !t.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(EsiError::Value {
            path: path.to_owned(),
            span: None,
            reason: format!("invalid hex byte string `{t}`"),
        });
    }
    Ok(t.as_bytes()
        .chunks(2)
        .map(|pair| {
            let s = std::str::from_utf8(pair).expect("ascii hex digits");
            u8::from_str_radix(s, 16).expect("validated hex digits")
        })
        .collect())
}

// ── eeprom conversion ─────────────────────────────────────────────────────────

/// Convert an [`EepromDto`]. `categories` is left empty here; the raw-xml
/// pass in [`crate::parse`] fills it (serde cannot capture unknown children).
fn eeprom_from_dto(dto: &EepromDto) -> Result<crate::model::Eeprom, EsiError> {
    Ok(crate::model::Eeprom {
        byte_size: dto
            .byte_size
            .as_deref()
            .map(|s| parse_esi_uint(s, "Eeprom.ByteSize"))
            .transpose()?,
        config_data: dto
            .config_data
            .as_deref()
            .map(|s| parse_hex_bytes(s, "Eeprom.ConfigData"))
            .transpose()?
            .unwrap_or_default(),
        bootstrap: dto
            .boot_strap
            .as_deref()
            .map(|s| parse_hex_bytes(s, "Eeprom.BootStrap"))
            .transpose()?,
        categories: Vec::new(),
    })
}

// ── MDP module / slot conversion ──────────────────────────────────────────────

fn module_from_dto(dto: ModuleDto) -> Result<Module, EsiError> {
    let mut tx_pdos = Vec::with_capacity(dto.tx_pdo.len());
    for p in dto.tx_pdo {
        tx_pdos.push(pdo_from_dto(p)?);
    }
    let mut rx_pdos = Vec::with_capacity(dto.rx_pdo.len());
    for p in dto.rx_pdo {
        rx_pdos.push(pdo_from_dto(p)?);
    }
    Ok(Module {
        ident: dto
            .ty
            .module_ident
            .as_deref()
            .map(|s| parse_esi_uint(s, "Module.Type.ModuleIdent"))
            .transpose()?,
        product_type: dto.ty.text,
        name: pick_name(&dto.names),
        tx_pdos,
        rx_pdos,
    })
}

fn slots_from_dto(dto: SlotsDto) -> Result<Slots, EsiError> {
    let mut slots = Vec::with_capacity(dto.slot.len());
    for s in dto.slot {
        let mut module_idents = Vec::with_capacity(s.module_ident.len());
        for mi in &s.module_ident {
            let raw = mi.text.as_deref().unwrap_or_default();
            module_idents.push(SlotModuleIdent {
                ident: parse_esi_uint(raw, "Slot.ModuleIdent")?,
                default: parse_esi_bool(mi.default.as_ref()),
            });
        }
        slots.push(Slot {
            name: pick_name(&s.names),
            min_instances: s
                .min_instances
                .as_deref()
                .map(|v| parse_esi_uint(v, "Slot.MinInstances"))
                .transpose()?,
            max_instances: s
                .max_instances
                .as_deref()
                .map(|v| parse_esi_uint(v, "Slot.MaxInstances"))
                .transpose()?,
            module_idents,
        });
    }
    Ok(Slots {
        slot_pdo_increment: dto
            .slot_pdo_increment
            .as_deref()
            .map(|v| parse_esi_uint(v, "Slots.SlotPdoIncrement"))
            .transpose()?,
        slot_index_increment: dto
            .slot_index_increment
            .as_deref()
            .map(|v| parse_esi_uint(v, "Slots.SlotIndexIncrement"))
            .transpose()?,
        slots,
    })
}

fn mailbox_from_dtos(
    mb: Option<MailboxDto>,
    profile: Option<&ProfileDto>,
) -> Result<Option<Mailbox>, EsiError> {
    let init_cmds_dto = profile
        .and_then(|p| p.dictionary.as_ref())
        .and_then(|d| d.init_cmds.as_ref());

    if mb.is_none() && init_cmds_dto.is_none() {
        return Ok(None);
    }

    let mut out = Mailbox::default();
    if let Some(mb) = mb {
        out.eoe = mb.eoe.is_some();
        out.foe = mb.foe.is_some();
        out.soe = mb.soe.is_some();
        out.voe = mb.voe.is_some();
        out.coe = mb.coe.map(|c| CoeInfo {
            sdo_info: parse_esi_bool(c.sdo_info.as_ref()),
            pdo_assign: parse_esi_bool(c.pdo_assign.as_ref()),
            pdo_config: parse_esi_bool(c.pdo_config.as_ref()),
            complete_access: parse_esi_bool(c.complete_access.as_ref()),
        });
    }

    if let Some(cmds) = init_cmds_dto {
        for c in &cmds.init_cmd {
            let transition = match c.transition.first().map(String::as_str) {
                Some("IP") => Transition::Ip,
                Some("PS") => Transition::Ps,
                Some("SO") => Transition::So,
                _ => Transition::Other,
            };
            out.init_cmds.push(InitCmd {
                transition,
                index: parse_esi_u16(&c.index, "InitCmd.Index")?,
                sub_index: match &c.sub_index {
                    Some(s) => parse_esi_u8(s, "InitCmd.SubIndex")?,
                    None => 0,
                },
                data: parse_hex_payload(c.data.as_deref().unwrap_or("")),
                comment: c.comment.clone(),
            });
        }
    }
    Ok(Some(out))
}

// ── distributed clock conversion ──────────────────────────────────────────────

fn dc_from_dto(dto: DcDto) -> Result<DistributedClock, EsiError> {
    let mut op_modes = Vec::with_capacity(dto.op_mode.len());
    for op in dto.op_mode {
        let assign_activate = parse_esi_u16(&op.assign_activate, "Dc.OpMode.AssignActivate")?;
        let cycle_time_sync0 = op
            .cycle_time_sync0
            .and_then(|c| c.value)
            .as_deref()
            .and_then(|s| s.parse::<i32>().ok());
        let shift_time_sync0 = op
            .shift_time_sync0
            .as_deref()
            .and_then(|s| s.parse::<i32>().ok());
        let cycle_time_sync1 = op
            .cycle_time_sync1
            .and_then(|c| c.value)
            .as_deref()
            .and_then(|s| s.parse::<i32>().ok());
        let shift_time_sync1 = op
            .shift_time_sync1
            .as_deref()
            .and_then(|s| s.parse::<i32>().ok());
        op_modes.push(DcOpMode {
            name: op.name,
            desc: op.desc,
            assign_activate,
            cycle_time_sync0,
            shift_time_sync0,
            cycle_time_sync1,
            shift_time_sync1,
        });
    }
    Ok(DistributedClock { op_modes })
}

// ── object dictionary conversion ──────────────────────────────────────────────

/// Derive [`Access`] flags from an optional [`FlagsDto`].
///
/// - `"ro"` → read only
/// - `"wo"` → write only
/// - `"rw"` → read + write
/// - `PdoMapping` containing `'T'` or `'R'` → `pdo_mappable`
fn access_from_flags(f: Option<&FlagsDto>) -> taktora_fieldbus_od_core::Access {
    use taktora_fieldbus_od_core::Access;
    let (mut read, mut write, mut pdo_mappable) = (false, false, false);
    if let Some(f) = f {
        match f.access.as_deref().map(str::trim) {
            Some("ro") => read = true,
            Some("wo") => write = true,
            Some("rw") => {
                read = true;
                write = true;
            }
            _ => {}
        }
        if let Some(m) = f.pdo_mapping.as_deref() {
            pdo_mappable = m.contains('T') || m.contains('R');
        }
    }
    Access {
        read,
        write,
        pdo_mappable,
    }
}

/// Parse an optional bit-size string into `Option<u32>`.
fn bitsize_from_str(s: Option<&String>) -> Option<u32> {
    s.and_then(|x| x.trim().parse::<u32>().ok())
}

/// Build a single [`DictEntry`] from a leaf `<Object>` (no `<SubItem>` children).
///
/// Leaf objects are emitted at `sub_index = 0`.
fn dict_entry_from_leaf(index: u16, obj: &ObjectDto) -> taktora_fieldbus_od_core::DictEntry {
    taktora_fieldbus_od_core::DictEntry {
        index,
        sub_index: 0,
        name: obj.name.clone().unwrap_or_default(),
        data_type: obj
            .ty
            .as_deref()
            .map_or_else(|| DataType::Named(String::new()), DataType::parse_coe_name),
        bit_size: bitsize_from_str(obj.bit_size.as_ref()),
        access: access_from_flags(obj.flags.as_ref()),
        default: obj.info.as_ref().and_then(|i| i.default_data.clone()),
    }
}

/// Build one [`DictEntry`] per `<SubItem>` of a compound `<Object>`.
fn dict_entries_from_compound(
    index: u16,
    sub_items: &[SubItemDto],
) -> Result<Vec<taktora_fieldbus_od_core::DictEntry>, EsiError> {
    let mut out = Vec::with_capacity(sub_items.len());
    for si in sub_items {
        out.push(taktora_fieldbus_od_core::DictEntry {
            index,
            sub_index: match &si.sub_index {
                Some(s) => parse_esi_u8(s, "SubItem.SubIndex")?,
                None => 0,
            },
            name: si.name.clone().unwrap_or_default(),
            data_type: si
                .ty
                .as_deref()
                .map_or_else(|| DataType::Named(String::new()), DataType::parse_coe_name),
            bit_size: bitsize_from_str(si.bit_size.as_ref()),
            access: access_from_flags(si.flags.as_ref()),
            default: si.info.as_ref().and_then(|i| i.default_data.clone()),
        });
    }
    Ok(out)
}

/// Convert the `<Profile><Dictionary><Objects>` section of a device into a flat
/// list of [`DictEntry`] records, one per leaf object or per `<SubItem>`.
///
/// Returns an empty vec when the profile contains no `<Objects>` section.
fn dictionary_from_profile(
    profile: Option<&ProfileDto>,
) -> Result<Vec<taktora_fieldbus_od_core::DictEntry>, EsiError> {
    let Some(objects) = profile
        .and_then(|p| p.dictionary.as_ref())
        .and_then(|d| d.objects.as_ref())
    else {
        return Ok(Vec::new());
    };

    let mut out = Vec::new();
    for obj in &objects.object {
        let index = parse_esi_u16(&obj.index, "Object.Index")?;
        if obj.sub_item.is_empty() {
            out.push(dict_entry_from_leaf(index, obj));
        } else {
            out.extend(dict_entries_from_compound(index, &obj.sub_item)?);
        }
    }
    Ok(out)
}

// ── AlternativeSmMapping conversion ──────────────────────────────────────────

/// Convert the parsed `<Info><VendorSpecific><TwinCAT>` `AlternativeSmMapping`
/// DTOs into the public IR. Returns an empty Vec when the chain is absent.
fn alt_sm_mappings_from_info(
    info: Option<&DeviceInfoDto>,
) -> Result<Vec<AlternativeSmMapping>, EsiError> {
    let Some(tc) = info
        .and_then(|i| i.vendor_specific.as_ref())
        .and_then(|v| v.twin_cat.as_ref())
    else {
        return Ok(Vec::new());
    };
    let mut out = Vec::with_capacity(tc.alternative_sm_mapping.len());
    for m in &tc.alternative_sm_mapping {
        let mut sm_assignments = Vec::with_capacity(m.sm.len());
        for sm in &m.sm {
            let no = parse_esi_u8(&sm.no, "AlternativeSmMapping.Sm.No")?;
            let mut pdos = Vec::with_capacity(sm.pdo.len());
            for p in &sm.pdo {
                let index = parse_esi_u16(&p.index, "AlternativeSmMapping.Sm.Pdo")?;
                let channel_no = p
                    .channel_no
                    .as_deref()
                    .map(|c| parse_esi_uint(c, "AlternativeSmMapping.Sm.Pdo.ChannelNo"))
                    .transpose()?;
                pdos.push(AltPdoRef { index, channel_no });
            }
            sm_assignments.push(SmAssignment { sm: no, pdos });
        }
        out.push(AlternativeSmMapping {
            name: m.name.clone(),
            default: parse_esi_bool(m.default.as_ref()),
            sm_assignments,
        });
    }
    Ok(out)
}

// ── device conversion ─────────────────────────────────────────────────────────

/// Convert one [`DeviceDto`] into an [`EsiDevice`], stamping the file-level
/// `vendor_id` onto its [`Identity`]. A `<Type>` lacking `ProductCode`/`RevisionNo`
/// defaults those identity fields to 0 (see [`TypeDto`]). `vendor_extensions` is
/// left empty here; the raw-xml pass in [`crate::parse`] fills it.
fn device_from_dto(dev: DeviceDto, vendor_id: u32) -> Result<EsiDevice, EsiError> {
    let identity = Identity {
        vendor_id,
        product_code: match dev.ty.product_code.as_deref() {
            Some(pc) => parse_esi_uint(pc, "Device.Type.ProductCode")?,
            None => 0,
        },
        revision: match dev.ty.revision_no.as_deref() {
            Some(rev) => parse_esi_uint(rev, "Device.Type.RevisionNo")?,
            None => 0,
        },
    };

    let mut tx_pdos = Vec::with_capacity(dev.tx_pdo.len());
    for p in dev.tx_pdo {
        tx_pdos.push(pdo_from_dto(p)?);
    }
    let mut rx_pdos = Vec::with_capacity(dev.rx_pdo.len());
    for p in dev.rx_pdo {
        rx_pdos.push(pdo_from_dto(p)?);
    }

    // IMPORTANT: pass &dev.profile (borrow) — the dictionary pass also reads it.
    let mailbox = mailbox_from_dtos(dev.mailbox, dev.profile.as_ref())?;
    let dc = dev.dc.map(dc_from_dto).transpose()?;
    let alt_sm_mappings = alt_sm_mappings_from_info(dev.info.as_ref())?;

    Ok(EsiDevice {
        identity,
        name: pick_name(&dev.names),
        product_type: dev.ty.text,
        group_type: dev.group_type,
        fmmus: fmmus_from_dtos(dev.fmmu),
        sync_managers: sync_managers_from_dtos(dev.sm)?,
        tx_pdos,
        rx_pdos,
        mailbox,
        dc,
        dictionary: dictionary_from_profile(dev.profile.as_ref())?,
        eeprom: dev.eeprom.as_ref().map(eeprom_from_dto).transpose()?,
        slots: dev.slots.map(slots_from_dto).transpose()?,
        alt_sm_mappings,
        vendor_extensions: Vec::new(),
    })
}

// ── MDP module catalog conversion ─────────────────────────────────────────────

/// Convert the optional top-level `<Descriptions><Modules>` catalog into a flat
/// list of [`Module`]s. Returns an empty vec when no `<Modules>` section exists.
fn modules_from_dto(modules: Option<ModulesDto>) -> Result<Vec<Module>, EsiError> {
    let Some(m) = modules else {
        return Ok(Vec::new());
    };
    let mut out = Vec::with_capacity(m.module.len());
    for dto in m.module {
        out.push(module_from_dto(dto)?);
    }
    Ok(out)
}

// ── top-level conversion ──────────────────────────────────────────────────────

impl EtherCatInfo {
    pub fn into_model(self) -> Result<EsiFile, EsiError> {
        let vendor_id = parse_esi_uint(&self.vendor.id, "Vendor.Id")?;
        let vendor = Vendor {
            id: vendor_id,
            name: pick_name(&self.vendor.names),
        };

        let device_dtos = self
            .descriptions
            .devices
            .map(|d| d.device)
            .unwrap_or_default();
        let mut devices = Vec::with_capacity(device_dtos.len());
        for dev in device_dtos {
            devices.push(device_from_dto(dev, vendor_id)?);
        }

        let modules = modules_from_dto(self.descriptions.modules)?;

        Ok(EsiFile {
            vendor,
            devices,
            modules,
        })
    }
}
