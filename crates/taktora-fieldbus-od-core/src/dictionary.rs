//! Object-dictionary entry model.

use serde::{Deserialize, Serialize};

use crate::DataType;

/// Access rights for an object-dictionary entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Access {
    /// Entry is readable (SDO upload).
    pub read: bool,
    /// Entry is writable (SDO download).
    pub write: bool,
    /// Entry may be mapped into a PDO.
    pub pdo_mappable: bool,
}

/// One object-dictionary entry, identified by `(index, sub_index)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DictEntry {
    /// Object dictionary index.
    pub index: u16,
    /// Sub-index within the object.
    pub sub_index: u8,
    /// Human-readable entry name.
    pub name: String,
    /// Value type of the entry.
    pub data_type: DataType,
    /// Declared bit size, when present in the description.
    pub bit_size: Option<u32>,
    /// Access rights.
    pub access: Access,
    /// Default value, captured verbatim as text (not decoded).
    pub default: Option<String>,
}
