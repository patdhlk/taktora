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
    CoeInfo, DcOpMode, DistributedClock, EsiDevice, EsiFile, InitCmd, Mailbox, Pdo, PdoEntry,
    SmDirection, SyncManager, Transition, Vendor,
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
        // quick-xml 0.40 DeError does not reliably expose a byte offset on
        // the deserialize path; map to the start of the document. Task 12
        // refines syntax-error positions.
        span: position::LineIndex::new(xml).span(0),
        source,
    })?;
    info.into_model()
}
