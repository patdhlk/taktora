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

/// Expected device identity, used for verification.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Identity {
    /// Vendor identifier.
    pub vendor_id: u32,
    /// Product code.
    pub product_code: u32,
    /// Revision number.
    pub revision: u32,
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
        NetworkConfig, PdoEntry,
    };
    use core::time::Duration;
    use serde::Deserialize;
    use std::path::PathBuf;

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
    }

    #[derive(Deserialize)]
    struct DeviceInstanceDto {
        label: String,
        #[serde(default)]
        esi: Option<String>,
        #[serde(default)]
        pdos: PdosDto,
        #[serde(default)]
        identity: Option<Identity>,
        #[serde(default)]
        station_alias: Option<u16>,
        #[serde(default, rename = "address")]
        address_override: Option<u16>,
    }

    #[derive(Deserialize, Default)]
    struct PdosDto {
        #[serde(default)]
        rx: Vec<PdoEntry>,
        #[serde(default)]
        tx: Vec<PdoEntry>,
    }

    impl NetworkConfigDto {
        /// Convert into the IR, resolving any `esi:` device references
        /// against the filesystem (`REQ_0824`).
        pub fn resolve(self) -> Result<NetworkConfig, NetcfgError> {
            let devices = self
                .devices
                .into_iter()
                .map(DeviceInstanceDto::resolve)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(NetworkConfig {
                schema_version: self.schema_version,
                bus: self.bus.into(),
                devices,
                channels: self.channels,
            })
        }
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

    /// Convert an `taktora_ethercat_esi::PdoEntry` into a netcfg [`PdoEntry`].
    const fn convert_pdo(entry: &taktora_ethercat_esi::PdoEntry) -> PdoEntry {
        PdoEntry {
            index: entry.index,
            bit_offset: entry.bit_offset,
            bit_length: entry.bit_length,
        }
    }

    impl From<BusConfigDto> for BusConfig {
        fn from(dto: BusConfigDto) -> Self {
            Self {
                cycle_time: Duration::from_millis(dto.cycle_time_ms),
                distributed_clocks: dto.distributed_clocks,
                max_subdevices: dto.max_subdevices,
                max_pdi_bytes: dto.max_pdi_bytes,
                default_nic: dto.default_nic,
            }
        }
    }

    impl DeviceInstanceDto {
        /// Convert into a [`DeviceInstance`], resolving an `esi:` reference
        /// (read file, parse, convert PDOs, map identity) when present.
        ///
        /// Path resolution is via `std::fs` against the process CWD;
        /// resolving relative-to-the-yaml-file is build glue and deferred.
        fn resolve(self) -> Result<DeviceInstance, NetcfgError> {
            let Self {
                label,
                esi,
                pdos,
                identity,
                station_alias,
                address_override,
            } = self;

            let (source, identity) = match esi {
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

                    let rx: Vec<PdoEntry> = device.rx_pdos.iter().map(convert_pdo).collect();
                    let tx: Vec<PdoEntry> = device.tx_pdos.iter().map(convert_pdo).collect();
                    // If the device ALSO carries inline pdos:, the two
                    // descriptions must agree. The ESI is the source of
                    // truth, so an exact match is redundant-but-legal and a
                    // mismatch is a contradiction (REQ_0824).
                    let has_inline = !pdos.rx.is_empty() || !pdos.tx.is_empty();
                    if has_inline && (pdos.rx != rx || pdos.tx != tx) {
                        return Err(NetcfgError::EsiContradiction { label });
                    }
                    // Keep an explicit YAML identity; otherwise map the
                    // ESI identity into the device.
                    let identity = identity.or(Some(Identity {
                        vendor_id: device.identity.vendor_id,
                        product_code: device.identity.product_code,
                        revision: device.identity.revision,
                    }));
                    (DeviceSource::Esi { path, rx, tx }, identity)
                }
                None => (
                    DeviceSource::Inline {
                        rx: pdos.rx,
                        tx: pdos.tx,
                    },
                    identity,
                ),
            };

            Ok(DeviceInstance {
                label,
                source,
                identity,
                station_alias,
                address_override,
            })
        }
    }
}
