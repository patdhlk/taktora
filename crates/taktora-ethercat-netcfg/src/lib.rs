//! Parser and in-memory IR for the `EtherCAT` network-config YAML.
//!
//! This crate is the **parse layer** of the `taktora-ethercat-netcfg` codegen
//! toolchain (a build-host tool — `std` is fine). It turns a
//! `network.yaml` document into the [`NetworkConfig`] IR via the single
//! public entry point [`parse`].
//!
//! It performs *parsing only*. Validation, address assignment, multi-bus
//! handling, and code generation are deliberately **out of scope** here —
//! they are handled by later layers of the toolchain.

#![warn(missing_docs)]

use core::time::Duration;

use serde::Deserialize;

pub use taktora_fieldbus_od_core::Identity;

/// The fully parsed network configuration — the IR root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkConfig {
    /// Schema version of the source document.
    pub schema_version: u16,
    /// Bus-wide configuration.
    pub bus: BusConfig,
    /// Device instances declared on the bus.
    pub devices: Vec<DeviceInstance>,
    /// Process-data channel bindings.
    pub channels: Vec<ChannelBinding>,
}

/// Bus-wide configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BusConfig {
    /// Cyclic process-data period.
    pub cycle_time: Duration,
    /// Whether distributed clocks are enabled.
    pub distributed_clocks: bool,
    /// Upper bound on the number of subdevices.
    pub max_subdevices: usize,
    /// Upper bound on the process-data-image size, in bytes.
    pub max_pdi_bytes: usize,
    /// Default NIC to bind the bus to, if any.
    pub default_nic: Option<String>,
    /// Fault-Tolerant Time Interval: the budget within which a detected
    /// fault must reach the safe state (`AOU_0006` / `AFSR_0004`). The
    /// per-device SM watchdog (`AOU_0016`) is bounded at FTTI/2 so a
    /// silently-stopped master still drives outputs safe inside budget.
    /// Defaults to 100 ms when the YAML omits `ftti_ms`.
    pub ftti: Duration,
}

/// A single device instance declared on the bus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInstance {
    /// Human-readable label, unique within the config.
    pub label: String,
    /// Where the device's PDO description comes from.
    pub source: DeviceSource,
    /// Optional expected identity for verification.
    pub identity: Option<Identity>,
    /// Optional configured station alias.
    pub station_alias: Option<u16>,
    /// Optional explicit configured-address override.
    pub address_override: Option<u16>,
    /// Optional per-device SM-watchdog timeout override. Absent → the
    /// device inherits FTTI/2 (`REQ_0844`). YAML: `sm_watchdog_timeout_ms`.
    pub sm_watchdog_timeout: Option<Duration>,
    /// Explicit attestation, for [`DeviceSource::Inline`] output devices,
    /// that the device's SM watchdog is (will be) enabled (`REQ_0845`).
    /// Inline sources carry no ESI control byte to read the enable bit
    /// from, so the integrator must attest it. YAML: `sm_watchdog_enabled`.
    pub sm_watchdog_enabled: Option<bool>,
    /// Resolved SM-watchdog register values for this device, present iff
    /// the device carries output (rx) PDOs (`REQ_0844`). Codegen emits
    /// these via `SubDeviceMap::with_sm_watchdog`; input-only devices
    /// carry `None` and emit no watchdog.
    pub sm_watchdog: Option<SmWatchdogRegisters>,
    /// Operator-declared startup SDOs, in declaration order. Empty if none.
    pub startup_sdos: Vec<StartupSdoSpec>,
}

/// Resolved ESC SM-watchdog register values for one output device.
///
/// `divider` is register `0x0400` (the tick base) and `intervals` is
/// register `0x0420` (the SM-watchdog time, in ticks). A tick is
/// `40 ns × (divider + 2)`; netcfg always fixes the divider at
/// [`DEFAULT_WATCHDOG_DIVIDER`] (a 100 µs tick) and varies only the tick
/// count. These mirror the connector's
/// `taktora_connector_ethercat::SmWatchdog` value (deliberately not a
/// dependency — see [`sm_watchdog_intervals`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SmWatchdogRegisters {
    /// Watchdog divider register `0x0400`. Tick = `40 ns × (divider + 2)`.
    pub divider: u16,
    /// SM-watchdog time register `0x0420`, in ticks.
    pub intervals: u16,
}

/// Watchdog divider that yields a 100 µs tick: `40 ns × (2498 + 2) = 100 µs`.
///
/// Mirrors `taktora_connector_ethercat::DEFAULT_DIVIDER` (the ESC power-up
/// value); duplicated here so netcfg need not depend on the connector
/// runtime (`REQ_0824`).
pub const DEFAULT_WATCHDOG_DIVIDER: u16 = 2498;

/// Quantize a timeout in microseconds to a whole number of 100 µs watchdog
/// ticks: `intervals = ceil(timeout_us / 100)`, clamped to `1..=u16::MAX`.
///
/// This is a deliberate ~5-line duplicate of the connector's
/// `taktora_connector_ethercat::SmWatchdog::from_timeout_us` arithmetic —
/// a dependency on that heavy crate was rejected (`REQ_0824` /
/// `crates/taktora-connector-ethercat/src/watchdog.rs`). Quantization is
/// upward (ceil): a request that is not a whole multiple of 100 µs rounds
/// **up** to the next tick, so the effective timeout is `≥ timeout_us`.
/// Callers checking the FTTI/2 ceiling must compare against the QUANTIZED
/// effective window, not the request. `0 µs` clamps to one tick, never the
/// disabling 0-interval value.
#[must_use]
pub const fn sm_watchdog_intervals(timeout_us: u32) -> u16 {
    let ticks = timeout_us.div_ceil(100);
    if ticks < 1 {
        1
    } else if ticks > u16::MAX as u32 {
        u16::MAX
    } else {
        // This branch only runs when `ticks <= u16::MAX`; `try_from` is
        // not const, so cast. Provably lossless.
        #[allow(clippy::cast_possible_truncation)]
        {
            ticks as u16
        }
    }
}

impl SmWatchdogRegisters {
    /// Build the registers for a timeout in microseconds, fixing the
    /// divider at [`DEFAULT_WATCHDOG_DIVIDER`] (100 µs ticks).
    #[must_use]
    pub const fn from_timeout_us(timeout_us: u32) -> Self {
        Self {
            divider: DEFAULT_WATCHDOG_DIVIDER,
            intervals: sm_watchdog_intervals(timeout_us),
        }
    }

    /// Effective (quantized) watchdog window in nanoseconds:
    /// `40 ns × (divider + 2) × intervals`. Computed in `u64`; the worst
    /// case (`2500 × 65535 × 40`) is far inside `u64`.
    #[must_use]
    pub const fn effective_timeout_ns(&self) -> u64 {
        40 * (self.divider as u64 + 2) * self.intervals as u64
    }
}

/// The origin of a device's PDO description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceSource {
    /// PDOs described inline in the network config.
    Inline {
        /// Receive (output) PDO entries.
        rx: Vec<PdoEntry>,
        /// Transmit (input) PDO entries.
        tx: Vec<PdoEntry>,
    },
    /// PDOs resolved from a referenced ESI (`EtherCAT` Slave Information)
    /// file at parse time (`REQ_0824`).
    Esi {
        /// The referenced ESI file, as named in the network config.
        path: std::path::PathBuf,
        /// Receive (output) PDO entries, resolved from the ESI file.
        rx: Vec<PdoEntry>,
        /// Transmit (input) PDO entries, resolved from the ESI file.
        tx: Vec<PdoEntry>,
    },
}

impl DeviceSource {
    /// Receive (output) PDO entries, regardless of source variant.
    #[must_use]
    pub fn rx(&self) -> &[PdoEntry] {
        match self {
            Self::Inline { rx, .. } | Self::Esi { rx, .. } => rx,
        }
    }

    /// Transmit (input) PDO entries, regardless of source variant.
    #[must_use]
    pub fn tx(&self) -> &[PdoEntry] {
        match self {
            Self::Inline { tx, .. } | Self::Esi { tx, .. } => tx,
        }
    }
}

/// A single PDO entry within an inline device description.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PdoEntry {
    /// PDO index.
    pub index: u16,
    /// Bit offset within the PDO.
    pub bit_offset: u16,
    /// Bit length of the entry.
    pub bit_length: u16,
}

/// One operator-declared startup SDO, written in PRE-OP before PDO assignment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupSdoSpec {
    /// SDO object-dictionary index.
    pub index: u16,
    /// SDO object-dictionary subindex.
    pub subindex: u8,
    /// Typed value to write.
    pub value: SdoValueSpec,
}

/// A typed startup-SDO value. Mirrors `taktora_connector_ethercat::SdoValue`;
/// codegen emits the matching connector variant as text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SdoValueSpec {
    /// 8-bit unsigned.
    U8(u8),
    /// 16-bit unsigned.
    U16(u16),
    /// 32-bit unsigned.
    U32(u32),
    /// 8-bit signed.
    I8(i8),
    /// 16-bit signed.
    I16(i16),
    /// 32-bit signed.
    I32(i32),
}

/// Direction of a process-data channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PdoDirection {
    /// Receive (output, controller -> device).
    Rx,
    /// Transmit (input, device -> controller).
    Tx,
}

/// A binding from a named channel to a slice of a device's process data.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ChannelBinding {
    /// Channel name (e.g. a topic).
    pub name: String,
    /// Label of the bound device.
    pub device: String,
    /// Direction of the channel.
    pub direction: PdoDirection,
    /// Bit offset within the device's process image.
    pub bit_offset: u32,
    /// Bit length of the channel.
    pub bit_length: u16,
    /// Primitive element type of the channel.
    pub element_type: ElementType,
    /// Whether overlapping bit ranges are permitted for this channel.
    #[serde(default)]
    pub allow_overlap: bool,
}

/// Primitive inline element types for a channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ElementType {
    /// Unsigned 8-bit integer.
    U8,
    /// Unsigned 16-bit integer.
    U16,
    /// Unsigned 32-bit integer.
    U32,
    /// Unsigned 64-bit integer.
    U64,
}

/// Errors that can occur while [`parse`]-ing a network config.
#[derive(Debug, thiserror::Error)]
pub enum NetcfgError {
    /// The source document could not be deserialized from YAML.
    #[error("failed to parse network config YAML: {0}")]
    Yaml(#[from] serde_norway::Error),

    /// The input is a multi-document YAML stream, but one `network.yaml`
    /// describes exactly one bus (`REQ_0822` / `ADR_0096`).
    #[error("found {count} YAML documents; one network.yaml describes exactly one bus")]
    MultipleBuses {
        /// Number of `---`-separated documents found in the stream.
        count: usize,
    },

    /// A referenced ESI file could not be read from the filesystem.
    #[error("failed to read referenced ESI file: {0}")]
    Io(#[from] std::io::Error),

    /// A referenced ESI file could not be parsed.
    ///
    /// `taktora_ethercat_esi::EsiError` is a `no_std` error that only implements
    /// `Display` (not `std::error::Error`), so it is not threaded as a
    /// `#[source]`; the message embeds its display form.
    #[error("failed to parse referenced ESI file: {0}")]
    Esi(taktora_ethercat_esi::EsiError),

    /// A device declares both an `esi:` reference and inline `pdos:`, but the
    /// ESI-resolved entries disagree with the inline entries.
    ///
    /// The ESI and the inline description are both available only after ESI
    /// resolution, so the contradiction is detected here in `parse` rather
    /// than in the downstream codegen validation pass.
    #[error("device `{label}` declares an esi: reference that contradicts its inline pdos:")]
    EsiContradiction {
        /// Label of the offending device.
        label: String,
    },

    /// A referenced ESI file describes more than one device, so the parser
    /// cannot unambiguously pick one (multi-device selection is deferred).
    #[error("ESI file {path:?} describes {count} devices; cannot select unambiguously")]
    AmbiguousEsiDevice {
        /// The referenced ESI file.
        path: std::path::PathBuf,
        /// Number of devices found in the ESI file.
        count: usize,
    },

    /// `op_mode` named a mapping that does not exist on the device.
    #[error("device `{label}` op_mode `{requested}` not found; available: {}", if available.is_empty() { String::from("(none)") } else { available.join(", ") })]
    OpModeNotFound {
        /// Offending device label.
        label: String,
        /// The requested mode name.
        requested: String,
        /// Mapping names the device declares.
        available: Vec<String>,
    },
    /// `op_mode` was set on a device that has no selectable PDO mappings — either
    /// because no `esi:` reference was given at all, or because the referenced
    /// ESI declares no `AlternativeSmMapping`.
    #[error(
        "device `{label}` sets op_mode but has no selectable PDO mappings (no esi: reference, or the referenced ESI declares no AlternativeSmMapping)"
    )]
    OpModeOnFlatDevice {
        /// Offending device label.
        label: String,
    },
    /// `op_mode` omitted on a multi-mapping device with no Default=\"1\" mapping.
    #[error("device `{label}` omits op_mode and its ESI declares no default PDO mapping")]
    NoDefaultMapping {
        /// Offending device label.
        label: String,
    },
    /// A mapping references a PDO index present in neither rx nor tx PDOs.
    #[error("device `{label}` mapping references unknown PDO index {index:#06x}")]
    UnknownAssignmentPdo {
        /// Offending device label.
        label: String,
        /// The dangling PDO index.
        index: u16,
    },

    /// An `esi:` reference is a remote (`http://` / `https://`) URL. Builds
    /// are hermetic and never fetch over the network at parse time
    /// (`REQ_0834`); the ESI must be vendored locally and referenced as a
    /// local file (or `file://` URL).
    #[error(
        "esi reference `{reference}` is a remote URL; vendor it locally with `netcfg fetch` and reference the vendored file"
    )]
    RemoteEsiNotVendored {
        /// The offending remote reference, as named in the network config.
        reference: String,
    },

    /// An output (rx-carrying) device's resolved SM-watchdog timeout, once
    /// quantized to whole 100 µs ticks, exceeds FTTI/2 (`AOU_0016` /
    /// `REQ_0845`). The bound is checked against the QUANTIZED effective
    /// window, since `ceil` can push a value that was under the raw bound
    /// over it.
    #[error(
        "device `{label}` SM-watchdog effective timeout {effective_us} µs exceeds the FTTI/2 bound of {bound_us} µs"
    )]
    SmWatchdogTimeoutTooLong {
        /// Label of the offending output device.
        label: String,
        /// The quantized effective watchdog window, in microseconds.
        effective_us: u64,
        /// The FTTI/2 bound, in microseconds.
        bound_us: u64,
    },

    /// An ESI-sourced output device's process-data (output) sync
    /// manager(s) have the watchdog trigger DISABLED in the ESI control
    /// byte (`AOU_0016` / `REQ_0845`). The watchdog is the sole mechanism
    /// that drives outputs to their safe state on a silently-stopped
    /// master, so a disabled trigger is a config error.
    #[error(
        "device `{label}` is an output device but its ESI declares the SM watchdog trigger disabled on an output sync manager"
    )]
    SmWatchdogDisabled {
        /// Label of the offending output device.
        label: String,
    },

    /// An [`DeviceSource::Inline`] output device did not attest that its
    /// SM watchdog is enabled (`AOU_0016` / `REQ_0845`). Inline sources
    /// carry no ESI control byte to read the enable bit from, so the
    /// integrator must set `sm_watchdog_enabled: true` — or switch to an
    /// ESI source whose output SM declares the trigger enabled.
    #[error(
        "device `{label}` is an inline output device without an SM-watchdog enable attestation; set `sm_watchdog_enabled: true` or source it from an ESI whose output SM enables the watchdog"
    )]
    SmWatchdogNotAttested {
        /// Label of the offending output device.
        label: String,
    },

    /// A startup-SDO `value` does not fit its declared `type`.
    #[error(
        "device `{label}` startup SDO {index:#06x}:{subindex:#04x} value {value} out of range for type {ty}"
    )]
    SdoValueOutOfRange {
        /// Offending device label.
        label: String,
        /// SDO index.
        index: u16,
        /// SDO subindex.
        subindex: u8,
        /// Declared type name (e.g. "u16").
        ty: String,
        /// The out-of-range value as written.
        value: i64,
    },
}

impl From<taktora_ethercat_esi::EsiError> for NetcfgError {
    fn from(e: taktora_ethercat_esi::EsiError) -> Self {
        Self::Esi(e)
    }
}

/// Parse a network-config YAML document into the [`NetworkConfig`] IR.
pub fn parse(yaml: &str) -> Result<NetworkConfig, NetcfgError> {
    // One-file-one-bus: a YAML stream may hold more than one
    // `---`-separated document. `Deserializer::from_str` yields one
    // `Deserializer` per document, so count them before deserializing.
    let count = serde_norway::Deserializer::from_str(yaml).count();
    if count > 1 {
        return Err(NetcfgError::MultipleBuses { count });
    }

    let dto: dto::NetworkConfigDto = serde_norway::from_str(yaml)?;
    dto.resolve()
}

/// Private deserialization DTOs.
///
/// These mirror the on-disk YAML shape (including serde defaults and the
/// `cycle_time_ms` field) and convert into the public IR. Keeping them
/// separate lets the IR stay free of serde concerns like the
/// milliseconds-to-[`Duration`] conversion.
mod dto {
    use super::{
        BusConfig, ChannelBinding, DeviceInstance, DeviceSource, Identity, NetcfgError,
        NetworkConfig, PdoEntry, SdoValueSpec, SmWatchdogRegisters, StartupSdoSpec,
        sm_watchdog_intervals,
    };
    use core::time::Duration;
    use serde::Deserialize;
    use std::path::PathBuf;

    /// Default Fault-Tolerant Time Interval when `ftti_ms` is omitted, in
    /// milliseconds (`AOU_0006` — 100 ms).
    const DEFAULT_FTTI_MS: u64 = 100;

    const fn default_ftti_ms() -> u64 {
        DEFAULT_FTTI_MS
    }

    #[derive(Deserialize)]
    pub struct NetworkConfigDto {
        schema_version: u16,
        bus: BusConfigDto,
        #[serde(default)]
        devices: Vec<DeviceInstanceDto>,
        #[serde(default)]
        channels: Vec<ChannelBinding>,
    }

    #[derive(Deserialize)]
    struct BusConfigDto {
        cycle_time_ms: u64,
        distributed_clocks: bool,
        max_subdevices: usize,
        max_pdi_bytes: usize,
        #[serde(default)]
        default_nic: Option<String>,
        #[serde(default = "default_ftti_ms")]
        ftti_ms: u64,
    }

    #[derive(Deserialize)]
    struct DeviceInstanceDto {
        label: String,
        #[serde(default)]
        esi: Option<String>,
        #[serde(default)]
        op_mode: Option<String>,
        #[serde(default)]
        pdos: PdosDto,
        #[serde(default)]
        identity: Option<Identity>,
        #[serde(default)]
        station_alias: Option<u16>,
        #[serde(default, rename = "address")]
        address_override: Option<u16>,
        #[serde(default)]
        sm_watchdog_timeout_ms: Option<u64>,
        #[serde(default)]
        sm_watchdog_enabled: Option<bool>,
        #[serde(default)]
        startup_sdos: Vec<StartupSdoDto>,
    }

    #[derive(Deserialize, Default)]
    struct PdosDto {
        #[serde(default)]
        rx: Vec<PdoEntry>,
        #[serde(default)]
        tx: Vec<PdoEntry>,
    }

    #[derive(Deserialize, Clone, Copy)]
    #[serde(rename_all = "lowercase")]
    enum SdoTypeDto {
        U8,
        U16,
        U32,
        I8,
        I16,
        I32,
    }

    impl SdoTypeDto {
        const fn name(self) -> &'static str {
            match self {
                Self::U8 => "u8",
                Self::U16 => "u16",
                Self::U32 => "u32",
                Self::I8 => "i8",
                Self::I16 => "i16",
                Self::I32 => "i32",
            }
        }
    }

    #[derive(Deserialize)]
    struct StartupSdoDto {
        index: u16,
        subindex: u8,
        #[serde(rename = "type")]
        ty: SdoTypeDto,
        value: i64,
    }

    fn convert_startup_sdo(
        label: &str,
        dto: &StartupSdoDto,
    ) -> Result<StartupSdoSpec, NetcfgError> {
        let v = dto.value;
        let oor = || NetcfgError::SdoValueOutOfRange {
            label: label.to_owned(),
            index: dto.index,
            subindex: dto.subindex,
            ty: dto.ty.name().to_owned(),
            value: v,
        };
        let value = match dto.ty {
            SdoTypeDto::U8 => SdoValueSpec::U8(u8::try_from(v).map_err(|_| oor())?),
            SdoTypeDto::U16 => SdoValueSpec::U16(u16::try_from(v).map_err(|_| oor())?),
            SdoTypeDto::U32 => SdoValueSpec::U32(u32::try_from(v).map_err(|_| oor())?),
            SdoTypeDto::I8 => SdoValueSpec::I8(i8::try_from(v).map_err(|_| oor())?),
            SdoTypeDto::I16 => SdoValueSpec::I16(i16::try_from(v).map_err(|_| oor())?),
            SdoTypeDto::I32 => SdoValueSpec::I32(i32::try_from(v).map_err(|_| oor())?),
        };
        Ok(StartupSdoSpec {
            index: dto.index,
            subindex: dto.subindex,
            value,
        })
    }

    /// A resolved device plus the ESI-derived watchdog-enable evidence the
    /// watchdog pass needs but the public IR does not carry. For an
    /// ESI-sourced device, `esi_output_watchdog_enabled` is `Some(true)`
    /// iff every output (process-data-write) sync manager declares the
    /// watchdog trigger enabled, `Some(false)` if any output SM disables
    /// it, and `None` if the ESI declares no output SM at all. Inline
    /// devices carry `None` (their attestation lives on the instance).
    struct ResolvedDevice {
        device: DeviceInstance,
        esi_output_watchdog_enabled: Option<bool>,
    }

    impl NetworkConfigDto {
        /// Convert into the IR, resolving any `esi:` device references
        /// against the filesystem (`REQ_0824`), then resolving and
        /// validating each output device's SM watchdog (`REQ_0844` /
        /// `REQ_0845`).
        pub fn resolve(self) -> Result<NetworkConfig, NetcfgError> {
            let bus: BusConfig = self.bus.into();
            // FTTI/2 bound, in microseconds, against which every output
            // device's quantized watchdog window is checked.
            let ftti_half_us = u64::try_from(bus.ftti.as_micros() / 2)
                .expect("FTTI/2 in µs fits in u64 for any sane ms-granular FTTI");

            let mut devices = Vec::with_capacity(self.devices.len());
            for dto in self.devices {
                let ResolvedDevice {
                    mut device,
                    esi_output_watchdog_enabled,
                } = dto.resolve()?;
                resolve_and_validate_watchdog(
                    &mut device,
                    ftti_half_us,
                    esi_output_watchdog_enabled,
                )?;
                devices.push(device);
            }

            Ok(NetworkConfig {
                schema_version: self.schema_version,
                bus,
                devices,
                channels: self.channels,
            })
        }
    }

    /// Resolve the effective SM watchdog for an output (rx-carrying) device
    /// and validate it against `AOU_0016` (`REQ_0844` / `REQ_0845`).
    ///
    /// Input-only devices (no rx PDOs) carry no watchdog and skip every
    /// check. For an output device:
    /// 1. Effective timeout = the `sm_watchdog_timeout` override if present,
    ///    else FTTI/2.
    /// 2. Quantize to ESC registers (divider 2498, `ceil` ticks) — the SAME
    ///    arithmetic as the connector's `SmWatchdog`.
    /// 3. The QUANTIZED effective window must be ≤ FTTI/2 (ceil can push a
    ///    boundary value over the bound — that is why the quantized value is
    ///    checked, not the request).
    /// 4. The watchdog must be ENABLED: an ESI source's output SM(s) must
    ///    declare the trigger enabled; an inline source must attest
    ///    `sm_watchdog_enabled: true`.
    fn resolve_and_validate_watchdog(
        device: &mut DeviceInstance,
        ftti_half_us: u64,
        esi_output_watchdog_enabled: Option<bool>,
    ) -> Result<(), NetcfgError> {
        // Input-only devices are untouched: no rx PDOs, no watchdog.
        if device.source.rx().is_empty() {
            return Ok(());
        }

        // 1 + 2: effective timeout (override or FTTI/2), quantized.
        let timeout_us = device.sm_watchdog_timeout.map_or_else(
            || {
                u32::try_from(ftti_half_us)
                    .expect("FTTI/2 in µs fits in u32 for any sane ms-granular FTTI")
            },
            |d| {
                u32::try_from(d.as_micros()).expect("per-device watchdog timeout in µs fits in u32")
            },
        );
        let registers = SmWatchdogRegisters {
            divider: super::DEFAULT_WATCHDOG_DIVIDER,
            intervals: sm_watchdog_intervals(timeout_us),
        };

        // 3: the QUANTIZED effective window must be ≤ FTTI/2.
        let effective_us = registers.effective_timeout_ns() / 1_000;
        if effective_us > ftti_half_us {
            return Err(NetcfgError::SmWatchdogTimeoutTooLong {
                label: device.label.clone(),
                effective_us,
                bound_us: ftti_half_us,
            });
        }

        // 4: the watchdog must be enabled.
        match &device.source {
            DeviceSource::Esi { .. } => {
                // `Some(false)` (an output SM disables the trigger) or
                // `None` (the ESI declares no output SM for an rx-carrying
                // device) both fail the enable check.
                if esi_output_watchdog_enabled != Some(true) {
                    return Err(NetcfgError::SmWatchdogDisabled {
                        label: device.label.clone(),
                    });
                }
            }
            DeviceSource::Inline { .. } => {
                if device.sm_watchdog_enabled != Some(true) {
                    return Err(NetcfgError::SmWatchdogNotAttested {
                        label: device.label.clone(),
                    });
                }
            }
        }

        device.sm_watchdog = Some(registers);
        Ok(())
    }

    /// Resolve an `esi:` reference to a LOCAL filesystem path (`REQ_0834`).
    ///
    /// Scheme handling for hermetic builds:
    /// - `http://` / `https://` → [`NetcfgError::RemoteEsiNotVendored`]; no
    ///   network access — the ESI must be vendored locally.
    /// - `file://` → strip the scheme; the remainder is a local path. For the
    ///   common `file:///absolute/path` form this yields `/absolute/path`.
    /// - anything else → the reference is itself a local path (unchanged).
    fn esi_local_path(reference: &str) -> Result<PathBuf, NetcfgError> {
        if reference.starts_with("http://") || reference.starts_with("https://") {
            return Err(NetcfgError::RemoteEsiNotVendored {
                reference: reference.to_owned(),
            });
        }
        if let Some(rest) = reference.strip_prefix("file://") {
            return Ok(PathBuf::from(rest));
        }
        Ok(PathBuf::from(reference))
    }

    impl From<BusConfigDto> for BusConfig {
        fn from(dto: BusConfigDto) -> Self {
            Self {
                cycle_time: Duration::from_millis(dto.cycle_time_ms),
                distributed_clocks: dto.distributed_clocks,
                max_subdevices: dto.max_subdevices,
                max_pdi_bytes: dto.max_pdi_bytes,
                default_nic: dto.default_nic,
                ftti: Duration::from_millis(dto.ftti_ms),
            }
        }
    }

    /// Fold an ESI device's sync managers into the
    /// `esi_output_watchdog_enabled` evidence the watchdog pass consumes:
    /// `Some(true)` iff at least one output SM exists and every output SM
    /// declares the watchdog trigger enabled; `Some(false)` if any output
    /// SM disables it; `None` if there is no output SM at all.
    fn esi_output_watchdog_enabled(
        sync_managers: &[taktora_ethercat_esi::SyncManager],
    ) -> Option<bool> {
        let mut saw_output = false;
        for sm in sync_managers {
            if sm.direction == taktora_ethercat_esi::SmDirection::Output {
                saw_output = true;
                if !sm.watchdog_trigger_enable {
                    return Some(false);
                }
            }
        }
        saw_output.then_some(true)
    }

    impl DeviceInstanceDto {
        /// Convert into a [`DeviceInstance`], resolving an `esi:` reference
        /// (read file, parse, convert PDOs, map identity) when present, and
        /// capturing the ESI's output-SM watchdog-enable evidence.
        ///
        /// Path resolution is via `std::fs` against the process CWD;
        /// resolving relative-to-the-yaml-file is build glue and deferred.
        /// The resolved `sm_watchdog` is `None` here; the watchdog pass in
        /// [`NetworkConfigDto::resolve`] fills it for output devices.
        fn resolve(self) -> Result<ResolvedDevice, NetcfgError> {
            let Self {
                label,
                esi,
                op_mode,
                pdos,
                identity,
                station_alias,
                address_override,
                sm_watchdog_timeout_ms,
                sm_watchdog_enabled,
                startup_sdos,
            } = self;

            if esi.is_none() && op_mode.is_some() {
                return Err(NetcfgError::OpModeOnFlatDevice { label });
            }

            // Convert startup SDOs before `label` is moved into the DeviceInstance literal.
            let startup_sdos = startup_sdos
                .iter()
                .map(|s| convert_startup_sdo(&label, s))
                .collect::<Result<Vec<_>, _>>()?;

            let (source, identity, esi_output_watchdog_enabled) = match esi {
                Some(reference) => {
                    let path = esi_local_path(&reference)?;
                    let xml = std::fs::read_to_string(&path)?;
                    let esi_file = taktora_ethercat_esi::parse(&xml)?;
                    let count = esi_file.devices.len();
                    // Minimal selection: exactly one device, or it is
                    // ambiguous (identity-based selection is deferred).
                    if count != 1 {
                        return Err(NetcfgError::AmbiguousEsiDevice { path, count });
                    }
                    let device = esi_file
                        .devices
                        .into_iter()
                        .next()
                        .expect("checked count == 1");

                    // op_mode selects the PDO mapping and is not retained in
                    // the IR; the resolved PDOs in `assignment` are the only output.
                    let assignment =
                        device.resolve_assignment(op_mode.as_deref()).map_err(|e| {
                            use taktora_ethercat_esi::ResolveError as R;
                            match e {
                                R::NoAlternativeMappings => NetcfgError::OpModeOnFlatDevice {
                                    label: label.clone(),
                                },
                                R::MappingNotFound {
                                    requested,
                                    available,
                                } => NetcfgError::OpModeNotFound {
                                    label: label.clone(),
                                    requested,
                                    available,
                                },
                                R::NoDefaultMapping => NetcfgError::NoDefaultMapping {
                                    label: label.clone(),
                                },
                                R::UnknownAssignmentPdo { index } => {
                                    NetcfgError::UnknownAssignmentPdo {
                                        label: label.clone(),
                                        index,
                                    }
                                }
                            }
                        })?;
                    let to_entry = |e: &taktora_ethercat_esi::ResolvedPdoEntry| PdoEntry {
                        index: e.index,
                        bit_offset: e.bit_offset,
                        bit_length: e.bit_length,
                    };
                    let rx: Vec<PdoEntry> = assignment.rx.iter().map(to_entry).collect();
                    let tx: Vec<PdoEntry> = assignment.tx.iter().map(to_entry).collect();
                    // If the device ALSO carries inline pdos:, the two
                    // descriptions must agree. The ESI is the source of
                    // truth, so an exact match is redundant-but-legal and a
                    // mismatch is a contradiction (REQ_0824). The ESI side is
                    // the op_mode-resolved mapping (if op_mode is set), so
                    // inline pdos: must match the selected mapping's PDOs.
                    let has_inline = !pdos.rx.is_empty() || !pdos.tx.is_empty();
                    if has_inline && (pdos.rx != rx || pdos.tx != tx) {
                        return Err(NetcfgError::EsiContradiction { label });
                    }
                    let wd_enabled = esi_output_watchdog_enabled(&device.sync_managers);
                    // Keep an explicit YAML identity; otherwise carry the
                    // ESI identity into the device (same type — od-core Identity).
                    let identity = identity.or(Some(device.identity));
                    (DeviceSource::Esi { path, rx, tx }, identity, wd_enabled)
                }
                None => (
                    DeviceSource::Inline {
                        rx: pdos.rx,
                        tx: pdos.tx,
                    },
                    identity,
                    None,
                ),
            };

            Ok(ResolvedDevice {
                device: DeviceInstance {
                    label,
                    source,
                    identity,
                    station_alias,
                    address_override,
                    sm_watchdog_timeout: sm_watchdog_timeout_ms.map(Duration::from_millis),
                    sm_watchdog_enabled,
                    sm_watchdog: None,
                    startup_sdos,
                },
                esi_output_watchdog_enabled,
            })
        }
    }
}
