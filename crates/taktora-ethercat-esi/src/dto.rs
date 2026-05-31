//! Private serde-derive DTOs mirroring the ESI XML shape, plus conversion to
//! the public IR. Integral ESI fields arrive as `String` (because of the `#x`
//! hex form) and are converted via [`parse_esi_uint`].

use serde::Deserialize;
use taktora_fieldbus_od_core::Identity;

use crate::error::EsiError;
use crate::model::{EsiDevice, EsiFile, Vendor};

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
    name: Option<String>,
}

#[derive(Deserialize)]
struct Descriptions {
    #[serde(rename = "Devices")]
    devices: Devices,
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
    name: Option<String>,
    #[serde(rename = "GroupType", default)]
    group_type: Option<String>,
}

#[derive(Deserialize)]
struct TypeDto {
    #[serde(rename = "@ProductCode")]
    product_code: String,
    #[serde(rename = "@RevisionNo")]
    revision_no: String,
    #[serde(rename = "$text", default)]
    text: Option<String>,
}

impl EtherCatInfo {
    pub(crate) fn into_model(self) -> Result<EsiFile, EsiError> {
        let vendor_id = parse_esi_uint(&self.vendor.id, "Vendor.Id")?;
        let vendor = Vendor {
            id: vendor_id,
            name: self.vendor.name,
        };

        let mut devices = Vec::with_capacity(self.descriptions.devices.device.len());
        for dev in self.descriptions.devices.device {
            let identity = Identity {
                vendor_id,
                product_code: parse_esi_uint(&dev.ty.product_code, "Device.Type.ProductCode")?,
                revision: parse_esi_uint(&dev.ty.revision_no, "Device.Type.RevisionNo")?,
            };
            devices.push(EsiDevice {
                identity,
                name: dev.name,
                product_type: dev.ty.text,
                group_type: dev.group_type,
                sync_managers: Vec::new(),
                tx_pdos: Vec::new(),
                rx_pdos: Vec::new(),
                mailbox: None,
                dc: None,
                dictionary: Vec::new(),
                vendor_extensions: Vec::new(),
            });
        }
        Ok(EsiFile { vendor, devices })
    }
}
