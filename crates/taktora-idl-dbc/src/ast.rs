//! The DBC-shaped abstract syntax: a faithful model of the parsed file.
//!
//! These types mirror DBC grammar, not the message IR. The projection onto
//! [`taktora_idl_core`] happens in [`lower`](crate::lower).

/// Bit numbering / byte order of a signal's payload.
///
/// DBC encodes this as `@1` (Intel) or `@0` (Motorola) on the signal line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteOrder {
    /// `@1` — Intel / little-endian.
    LittleEndian,
    /// `@0` — Motorola / big-endian.
    BigEndian,
}

/// A signal's multiplexing role within its message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Multiplexer {
    /// Ordinary signal, always present.
    None,
    /// The multiplexor switch (`M`) selecting which multiplexed group is live.
    Multiplexor,
    /// A multiplexed signal (`m<n>`), present only when the switch equals `n`.
    Multiplexed(u16),
}

/// One signal (`SG_`) within a message.
#[derive(Debug, Clone, PartialEq)]
pub struct DbcSignal {
    /// Signal name.
    pub name: String,
    /// Multiplexing role.
    pub multiplexer: Multiplexer,
    /// Start bit position within the frame.
    pub start_bit: u16,
    /// Width in bits.
    pub bit_len: u16,
    /// Byte order of the raw value.
    pub byte_order: ByteOrder,
    /// Whether the raw value is two's-complement signed.
    pub signed: bool,
    /// Linear scaling factor (`physical = raw * factor + offset`).
    pub factor: f64,
    /// Linear scaling offset.
    pub offset: f64,
    /// Declared physical minimum.
    pub min: f64,
    /// Declared physical maximum.
    pub max: f64,
    /// Engineering unit (may be empty).
    pub unit: String,
    /// Receiving node names.
    pub receivers: Vec<String>,
}

/// One message (`BO_`): a CAN frame definition.
#[derive(Debug, Clone, PartialEq)]
pub struct DbcMessage {
    /// Raw 32-bit DBC message id. The high bit flags an extended (29-bit) id;
    /// see [`can_id`](Self::can_id) and [`is_extended`](Self::is_extended).
    pub id: u32,
    /// Message name.
    pub name: String,
    /// Data length in bytes (`DLC`).
    pub dlc: u8,
    /// Transmitting node name.
    pub transmitter: String,
    /// Signals carried by the frame.
    pub signals: Vec<DbcSignal>,
}

impl DbcMessage {
    /// The high bit of a DBC id marks a 29-bit extended frame.
    const EXTENDED_FLAG: u32 = 0x8000_0000;

    /// Whether this message uses an extended (29-bit) CAN identifier.
    #[must_use]
    pub const fn is_extended(&self) -> bool {
        self.id & Self::EXTENDED_FLAG != 0
    }

    /// The CAN identifier with the DBC extended-flag bit masked off.
    #[must_use]
    pub const fn can_id(&self) -> u32 {
        self.id & !Self::EXTENDED_FLAG
    }
}

/// A value table (`VAL_`): the enumerated meanings of a signal's raw values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbcValueTable {
    /// Message id the signal belongs to (raw DBC id, as on the `VAL_` line).
    pub message_id: u32,
    /// Signal name.
    pub signal: String,
    /// `(raw value, label)` pairs, in file order.
    pub entries: Vec<(i64, String)>,
}

/// A parsed DBC file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DbcDatabase {
    /// `VERSION "..."` string, if present.
    pub version: Option<String>,
    /// Node names from the `BU_:` line.
    pub nodes: Vec<String>,
    /// Message definitions, in file order.
    pub messages: Vec<DbcMessage>,
    /// Value tables, in file order.
    pub value_tables: Vec<DbcValueTable>,
}

impl DbcDatabase {
    /// Find the value table for a `(message id, signal)` pair, matching on the
    /// CAN id regardless of the extended-flag bit.
    #[must_use]
    pub fn value_table_for(&self, message_id: u32, signal: &str) -> Option<&DbcValueTable> {
        let want = message_id & !DbcMessage::EXTENDED_FLAG;
        self.value_tables
            .iter()
            .find(|t| t.message_id & !DbcMessage::EXTENDED_FLAG == want && t.signal == signal)
    }
}
