//! Primitive scalar types and their fixed wire footprint.

use serde::{Deserialize, Serialize};

/// A primitive value type with a fixed, known serialized size.
///
/// These are the leaves of every [`Type`](crate::Type): every bounded compound
/// ultimately decomposes into scalars, which is what makes the
/// [maximum serialized length](crate::Module::max_serialized_len) of any type
/// computable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Scalar {
    /// 1-byte boolean.
    Bool,
    /// Unsigned 8-bit.
    U8,
    /// Unsigned 16-bit.
    U16,
    /// Unsigned 32-bit.
    U32,
    /// Unsigned 64-bit.
    U64,
    /// Signed 8-bit.
    I8,
    /// Signed 16-bit.
    I16,
    /// Signed 32-bit.
    I32,
    /// Signed 64-bit.
    I64,
    /// IEEE-754 32-bit float.
    F32,
    /// IEEE-754 64-bit float.
    F64,
}

impl Scalar {
    /// Size of one value of this scalar in bytes, before any framing.
    #[must_use]
    pub const fn wire_size(self) -> usize {
        match self {
            Self::Bool | Self::U8 | Self::I8 => 1,
            Self::U16 | Self::I16 => 2,
            Self::U32 | Self::I32 | Self::F32 => 4,
            Self::U64 | Self::I64 | Self::F64 => 8,
        }
    }

    /// Width of this scalar in bits.
    #[must_use]
    pub const fn bit_width(self) -> u16 {
        match self {
            Self::Bool | Self::U8 | Self::I8 => 8,
            Self::U16 | Self::I16 => 16,
            Self::U32 | Self::I32 | Self::F32 => 32,
            Self::U64 | Self::I64 | Self::F64 => 64,
        }
    }

    /// Whether this scalar is a signed integer.
    #[must_use]
    pub const fn is_signed_integer(self) -> bool {
        matches!(self, Self::I8 | Self::I16 | Self::I32 | Self::I64)
    }

    /// Whether this scalar is an IEEE-754 float.
    #[must_use]
    pub const fn is_float(self) -> bool {
        matches!(self, Self::F32 | Self::F64)
    }

    /// The narrowest integer scalar that holds a value of `bits` width.
    ///
    /// Returns the unsigned variant when `signed` is `false`. Widths are
    /// rounded up to the next power-of-two byte boundary (1, 2, 4, 8 bytes);
    /// `bits` greater than 64 yields `None`.
    #[must_use]
    pub const fn integer_for_bits(bits: u16, signed: bool) -> Option<Self> {
        match (bits, signed) {
            (1..=8, false) => Some(Self::U8),
            (1..=8, true) => Some(Self::I8),
            (9..=16, false) => Some(Self::U16),
            (9..=16, true) => Some(Self::I16),
            (17..=32, false) => Some(Self::U32),
            (17..=32, true) => Some(Self::I32),
            (33..=64, false) => Some(Self::U64),
            (33..=64, true) => Some(Self::I64),
            _ => None,
        }
    }
}
