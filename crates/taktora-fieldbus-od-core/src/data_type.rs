//! Fieldbus value data types (`CoE` / `CANopen`).

use serde::{Deserialize, Serialize};

/// A fieldbus value type, as named in ESI/EDS device descriptions.
///
/// Known `CoE` base types map to dedicated variants; bit-width types map to
/// [`DataType::BitN`]; anything else (complex / vendor / unrecognised) is
/// preserved verbatim in [`DataType::Named`] rather than rejected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataType {
    /// `BOOL`.
    Bool,
    /// `SINT` — signed 8-bit.
    I8,
    /// `INT` — signed 16-bit.
    I16,
    /// `DINT` — signed 32-bit.
    I32,
    /// `LINT` — signed 64-bit.
    I64,
    /// `USINT` — unsigned 8-bit.
    U8,
    /// `UINT` — unsigned 16-bit.
    U16,
    /// `UDINT` — unsigned 32-bit.
    U32,
    /// `ULINT` — unsigned 64-bit.
    U64,
    /// `REAL` — IEEE-754 32-bit.
    Real32,
    /// `LREAL` — IEEE-754 64-bit.
    Real64,
    /// `BITn` — a sub-byte bit field of the given width (1..=8).
    BitN(u8),
    /// `STRING(n)` — visible (8-bit) string.
    VisibleString,
    /// Octet string.
    OctetString,
    /// Unicode string.
    UnicodeString,
    /// A complex, vendor, or unrecognised type, preserved by its ESI name.
    Named(String),
}

impl DataType {
    /// Parse a `BIT`-prefixed width suffix, returning `Some(BitN(w))` for widths 1–8.
    fn parse_bit_width(upper: &str) -> Option<Self> {
        let w = upper.strip_prefix("BIT")?.parse::<u8>().ok()?;
        (1..=8).contains(&w).then_some(Self::BitN(w))
    }

    /// Map an ESI/CoE type name (the text of an ESI `<Type>` / `BaseType`) to a
    /// [`DataType`]. Unknown names are returned as [`DataType::Named`].
    #[must_use]
    pub fn parse_coe_name(name: &str) -> Self {
        let trimmed = name.trim();
        let upper = trimmed.to_ascii_uppercase();
        match upper.as_str() {
            "BOOL" | "BOOLEAN" => Self::Bool,
            "SINT" => Self::I8,
            "INT" => Self::I16,
            "DINT" => Self::I32,
            "LINT" => Self::I64,
            "USINT" | "BYTE" => Self::U8,
            "UINT" | "WORD" => Self::U16,
            "UDINT" | "DWORD" => Self::U32,
            "ULINT" | "LWORD" => Self::U64,
            "REAL" => Self::Real32,
            "LREAL" => Self::Real64,
            "OCTETSTRING" => Self::OctetString,
            "UNICODESTRING" => Self::UnicodeString,
            s if s == "STRING" || s == "VISIBLESTRING" || s.starts_with("STRING(") => {
                Self::VisibleString
            }
            _ => Self::parse_bit_width(&upper).unwrap_or_else(|| Self::Named(trimmed.to_owned())),
        }
    }
}
