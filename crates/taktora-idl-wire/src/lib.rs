//! Serde-free runtime support for generated message (de)serializers.
//!
//! This is the runtime half of the message-plane codegen (`FEAT_0100`). The
//! codegen emits one [`WireType`] implementation per message; that generated
//! code calls *only* the primitives in this crate. The split exists so the
//! safety-relevant (de)serialization path is `no_std`, allocation-free, and
//! free of `serde`/reflection (`REQ_0861`) — small enough to keep
//! Kani/Miri-verifiable — while the policy-heavy emission stays host-side.
//!
//! # Contents
//!
//! * [`WireType`] — the `encode`/`decode` contract every generated message
//!   type implements (`REQ_0860`).
//! * [`WireError`] — the closed error set the path can produce.
//! * [`ByteOrder`] and the [`pack_unsigned`] / [`pack_signed`] /
//!   [`unpack_unsigned`] / [`unpack_signed`] free functions — CAN signal
//!   bit-packing, addressing the two DBC bit-numbering conventions (`REQ_0862`).
//!
//! The bit primitives take an explicit frame slice and never allocate; a
//! generated `encode` zeroes its `DLC` bytes and packs each signal in place.

#![cfg_attr(not(test), no_std)]

mod bits;
mod error;

pub use bits::{ByteOrder, pack_signed, pack_unsigned, unpack_signed, unpack_unsigned};
pub use error::WireError;

/// A type that serializes to and from a fixed-layout byte buffer without serde.
///
/// Implementations are generated from a description (e.g. a DBC message). The
/// contract is deliberately minimal so it can be audited and model-checked:
/// fixed upper bound, in-place encode, owned decode.
pub trait WireType: Sized {
    /// Upper bound, in bytes, on a serialized value of this type: the fixed
    /// wire length the backend frames into. A buffer of this length is always
    /// large enough for [`encode`](WireType::encode), and `decode` requires at
    /// least this many bytes.
    ///
    /// This is the backend's wire footprint and need not equal the `idl-core`
    /// [`max_serialized_len`] of the source type (`REQ_0865`): a backend that
    /// frames into a fixed envelope reports that envelope. For the CAN backend
    /// it is the message `DLC` — the on-wire frame length — which can exceed the
    /// packed extent of its signals.
    ///
    /// [`max_serialized_len`]: https://docs.rs/taktora-idl-core
    const MAX_SERIALIZED_LEN: usize;

    /// Serialize `self` into the start of `buf`, returning the number of bytes
    /// written.
    ///
    /// # Errors
    ///
    /// [`WireError::BufferTooSmall`] if `buf.len() < MAX_SERIALIZED_LEN`.
    fn encode(&self, buf: &mut [u8]) -> Result<usize, WireError>;

    /// Deserialize a value from the start of `buf`.
    ///
    /// # Errors
    ///
    /// [`WireError::BufferTooSmall`] if `buf` is shorter than the type's wire
    /// length, or [`WireError::UnknownEnumValue`] if an enum-typed field holds
    /// a raw value with no defined variant.
    fn decode(buf: &[u8]) -> Result<Self, WireError>;
}
