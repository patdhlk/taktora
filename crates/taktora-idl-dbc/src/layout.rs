//! The CAN signal-layout sidecar.
//!
//! This is the DBC-*specific* half of a lowering: the physical bit-packing and
//! linear scaling that a CAN-frame `WireType` needs but that has no place in
//! the plane-generic [`idl_core::Module`](taktora_idl_core::Module). Each entry
//! is keyed by the same names the module uses, so a backend can join the two.

use crate::ast::ByteOrder;

/// Physical placement and scaling of one signal within its frame.
#[derive(Debug, Clone, PartialEq)]
pub struct SignalLayout {
    /// The `idl_core` field name this layout describes.
    pub field_name: String,
    /// Start bit within the frame.
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
}

/// Physical layout of one CAN frame: identity, length, and its signals.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameLayout {
    /// The `idl_core` struct name this frame lowers to.
    pub struct_name: String,
    /// CAN identifier (extended-flag bit masked off).
    pub can_id: u32,
    /// Whether the frame uses a 29-bit extended identifier.
    pub extended: bool,
    /// Data length in bytes.
    pub dlc: u8,
    /// Per-signal layout, in struct-field order.
    pub signals: Vec<SignalLayout>,
}

/// The complete CAN-layout sidecar for a lowered DBC module.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DbcLayout {
    /// Frame layouts, in module-struct order.
    pub frames: Vec<FrameLayout>,
}

impl DbcLayout {
    /// Find the layout for a given lowered struct name.
    #[must_use]
    pub fn frame(&self, struct_name: &str) -> Option<&FrameLayout> {
        self.frames.iter().find(|f| f.struct_name == struct_name)
    }
}
