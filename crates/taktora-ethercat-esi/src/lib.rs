//! Parser and faithful IR for `EtherCAT` ESI (`EtherCAT` Slave Information) XML.
//!
//! Turns an ESI XML string into a typed in-memory IR. Performs no filesystem
//! or network I/O — the caller supplies the XML string. The IR is *faithful*:
//! it records what the document declares (including PDO assignment
//! alternatives and padding entries) without resolving a process image.

#![warn(missing_docs)]

mod dto;
mod error;
mod model;
mod position;
mod raw_xml;

pub use error::{EsiError, Span};
pub use model::{
    CoeInfo, DcOpMode, DistributedClock, Eeprom, EsiDevice, EsiFile, Fmmu, FmmuUsage, InitCmd,
    Mailbox, Module, Pdo, PdoEntry, Slot, SlotModuleIdent, Slots, SmDirection, SyncManager,
    Transition, Vendor,
};
pub use raw_xml::RawXml;
pub use taktora_fieldbus_od_core::{Access, DataType, DictEntry, Identity};

/// Parse an ESI XML document into an [`EsiFile`].
///
/// # Errors
///
/// Returns [`EsiError::Xml`] if the document is syntactically invalid, or
/// [`EsiError::Number`] / [`EsiError::Value`] if a field value cannot be
/// interpreted.
pub fn parse(xml: &str) -> Result<EsiFile, EsiError> {
    let info: dto::EtherCatInfo = quick_xml::de::from_str(xml).map_err(|source| EsiError::Xml {
        // quick-xml 0.40 DeError does not carry a usable byte offset on the
        // deserialize path. Re-walk the document with the low-level reader to
        // recover a real line/column for the failing token; fall back to the
        // document start if the reader finds no syntax fault.
        span: locate_syntax_error(xml),
        source,
    })?;
    let mut file = info.into_model()?;
    let exts = raw_xml::capture_device_extensions(xml)?;
    for (dev, ext) in file.devices.iter_mut().zip(exts) {
        dev.vendor_extensions = ext;
    }
    let cats = raw_xml::capture_eeprom_categories(xml)?;
    for (dev, cats) in file.devices.iter_mut().zip(cats) {
        if let Some(eeprom) = dev.eeprom.as_mut() {
            eeprom.categories = cats;
        }
    }
    Ok(file)
}

/// Re-walk the XML with the low-level `quick_xml::reader::Reader` to find the
/// byte offset of the first syntax fault, mapped to a 1-based [`Span`]. quick-xml
/// 0.40's `error_position()` returns a `u64` byte offset pointing at the start of
/// the offending token. If no fault is found (deserialize failed for a
/// non-syntax reason), returns the document start.
fn locate_syntax_error(xml: &str) -> Span {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let index = position::LineIndex::new(xml);
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Eof) => return index.span(0),
            Ok(_) => {}
            Err(_) => {
                let offset = usize::try_from(reader.error_position()).unwrap_or(usize::MAX);
                return index.span(offset);
            }
        }
    }
}
