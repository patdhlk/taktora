//! `no_std` parser for `EtherCAT` ESI (`EtherCAT` Slave Information) XML.
//!
//! This crate turns an ESI XML string into a typed in-memory IR. It performs
//! no filesystem or network I/O — the caller supplies the XML string.
//!
//! Minimal slice: per device, the [`Identity`] plus the flattened `TxPDO`
//! [`PdoEntry`] list.

#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use core::fmt;

/// A parsed ESI file: one or more devices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EsiFile {
    /// Devices described by the ESI file, in document order.
    pub devices: Vec<EsiDevice>,
}

/// A single `EtherCAT` device description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EsiDevice {
    /// Device identity (vendor / product / revision).
    pub identity: Identity,
    /// Flattened `TxPDO` entries, in document order.
    pub tx_pdos: Vec<PdoEntry>,
    /// Flattened `RxPDO` entries (parsed in a later slice; empty for now).
    pub rx_pdos: Vec<PdoEntry>,
}

/// The vendor / product / revision triple identifying a device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    /// `EtherCAT` vendor id.
    pub vendor_id: u32,
    /// Device product code.
    pub product_code: u32,
    /// Device revision number.
    pub revision: u32,
}

/// A single process-data object entry mapped into the I/O image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdoEntry {
    /// Object dictionary index of the entry.
    pub index: u16,
    /// Cumulative bit offset within the device's PDO image.
    pub bit_offset: u16,
    /// Length of the entry in bits.
    pub bit_length: u16,
}

/// Errors that can occur while parsing an ESI file.
#[derive(Debug)]
pub enum EsiError {
    /// The XML could not be deserialized.
    Xml(quick_xml::de::DeError),
    /// An ESI integer field could not be parsed.
    Number,
}

impl fmt::Display for EsiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Xml(e) => write!(f, "failed to parse ESI XML: {e}"),
            Self::Number => f.write_str("invalid ESI integer"),
        }
    }
}

impl From<quick_xml::de::DeError> for EsiError {
    fn from(e: quick_xml::de::DeError) -> Self {
        Self::Xml(e)
    }
}

/// Parse an ESI integer.
///
/// ESI numbers are either hex with a `#x` / `#X` prefix (e.g. `#x1a00`) or
/// plain decimal (e.g. `8`). Anything else is an [`EsiError::Number`].
fn parse_esi_uint(raw: &str) -> Result<u32, EsiError> {
    let raw = raw.trim();
    let parsed = raw
        .strip_prefix("#x")
        .or_else(|| raw.strip_prefix("#X"))
        .map_or_else(
            || raw.parse::<u32>().ok(),
            |hex| u32::from_str_radix(hex, 16).ok(),
        );
    parsed.ok_or(EsiError::Number)
}

/// Private DTO module mirroring the raw ESI XML shape. All integral fields are
/// captured as `String` (because of the `#x` hex format) and converted via
/// [`parse_esi_uint`] in [`dto::EtherCatInfo::into_esi`].
mod dto {
    use super::{EsiDevice, EsiError, EsiFile, Identity, PdoEntry, Vec, parse_esi_uint};
    use alloc::string::String;
    use serde::Deserialize;

    #[derive(Deserialize)]
    pub struct EtherCatInfo {
        #[serde(rename = "Vendor")]
        vendor: Vendor,
        #[serde(rename = "Descriptions")]
        descriptions: Descriptions,
    }

    #[derive(Deserialize)]
    struct Vendor {
        #[serde(rename = "Id")]
        id: String,
    }

    #[derive(Deserialize)]
    struct Descriptions {
        #[serde(rename = "Devices")]
        devices: Devices,
    }

    #[derive(Deserialize)]
    struct Devices {
        #[serde(rename = "Device", default)]
        device: Vec<Device>,
    }

    #[derive(Deserialize)]
    struct Device {
        #[serde(rename = "Type")]
        ty: Type,
        #[serde(rename = "TxPdo", default)]
        tx_pdo: Vec<Pdo>,
    }

    #[derive(Deserialize)]
    struct Type {
        #[serde(rename = "@ProductCode")]
        product_code: String,
        #[serde(rename = "@RevisionNo")]
        revision_no: String,
    }

    #[derive(Deserialize)]
    struct Pdo {
        #[serde(rename = "Entry", default)]
        entry: Vec<Entry>,
    }

    #[derive(Deserialize)]
    struct Entry {
        #[serde(rename = "Index")]
        index: String,
        #[serde(rename = "BitLen")]
        bit_len: String,
    }

    impl EtherCatInfo {
        pub fn into_esi(self) -> Result<EsiFile, EsiError> {
            let vendor_id = parse_esi_uint(&self.vendor.id)?;

            let mut devices = Vec::with_capacity(self.descriptions.devices.device.len());
            for dev in self.descriptions.devices.device {
                let identity = Identity {
                    vendor_id,
                    product_code: parse_esi_uint(&dev.ty.product_code)?,
                    revision: parse_esi_uint(&dev.ty.revision_no)?,
                };

                let mut tx_pdos = Vec::new();
                let mut bit_offset: u16 = 0;
                for pdo in dev.tx_pdo {
                    for entry in pdo.entry {
                        let index = parse_esi_uint(&entry.index)?
                            .try_into()
                            .map_err(|_| EsiError::Number)?;
                        let bit_length: u16 = parse_esi_uint(&entry.bit_len)?
                            .try_into()
                            .map_err(|_| EsiError::Number)?;
                        tx_pdos.push(PdoEntry {
                            index,
                            bit_offset,
                            bit_length,
                        });
                        bit_offset = bit_offset.saturating_add(bit_length);
                    }
                }

                devices.push(EsiDevice {
                    identity,
                    tx_pdos,
                    rx_pdos: Vec::new(),
                });
            }

            Ok(EsiFile { devices })
        }
    }
}

/// Parse an ESI XML document into an [`EsiFile`].
pub fn parse(xml: &str) -> Result<EsiFile, EsiError> {
    let info: dto::EtherCatInfo = quick_xml::de::from_str(xml)?;
    info.into_esi()
}
