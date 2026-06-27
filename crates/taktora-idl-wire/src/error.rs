//! The closed error set of the serialization path.

use core::fmt;

/// An error from a [`WireType`](crate::WireType) operation or a bit-packing
/// primitive. Hand-rolled (no `thiserror`) to keep this crate dependency-free
/// and `no_std` (`REQ_0861`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum WireError {
    /// The destination buffer is smaller than the value's wire length.
    BufferTooSmall,
    /// A signal's bit range falls outside the frame.
    SignalOutOfBounds,
    /// A bit length outside the supported `1..=64` range was requested.
    InvalidBitLength,
    /// A value does not fit the signal's bit width (it would be truncated).
    ValueOutOfRange,
    /// An enum-typed field decoded to a raw value with no defined variant.
    UnknownEnumValue,
}

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = match self {
            Self::BufferTooSmall => "destination buffer smaller than wire length",
            Self::SignalOutOfBounds => "signal bit range falls outside the frame",
            Self::InvalidBitLength => "bit length outside the supported 1..=64 range",
            Self::ValueOutOfRange => "value does not fit the signal's bit width",
            Self::UnknownEnumValue => "enum field decoded to an undefined variant",
        };
        f.write_str(msg)
    }
}

impl core::error::Error for WireError {}
