//! [`BinaryCodec`] — fixed-width binary `PayloadCodec` backed by
//! `bincode`. ``REQ_0212``, ``BB_0003``.
//!
//! Uses `bincode`'s serde integration with **fixed-int encoding** and a
//! **selectable byte order** so that fixed-width primitives encode to a
//! constant number of bytes (a `u16` is always 2 bytes, never bincode's
//! varint 1–3). That constant width is the point: a routing slice's
//! `bit_length` can then be a static constant. The default is
//! big-endian (network / EtherCAT-PDI byte order).
//!
//! Encode writes directly into the caller-provided buffer via
//! [`bincode::serde::encode_into_slice`], so a successful encode does
//! not allocate. Buffer-too-small surfaces as
//! [`ConnectorError::PayloadOverflow`] for consistency with
//! `ChannelWriter::send`'s overflow behaviour (`TEST_0125`); other
//! serializer failures surface as [`ConnectorError::Codec`] (`REQ_0213`).
//!
//! Decode delegates to [`bincode::serde::decode_from_slice`]; failures
//! (truncated input, schema mismatch) surface as
//! [`ConnectorError::Codec`] (`REQ_0214`) rather than being silently
//! dropped.

use bincode::config::Config;
use taktora_connector_core::{ConnectorError, PayloadCodec};

/// Byte order used when encoding fixed-width integers.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum Endian {
    /// Big-endian — network / EtherCAT-PDI byte order. The default.
    #[default]
    Big,
    /// Little-endian.
    Little,
}

/// Fixed-width binary codec built on `bincode`'s serde integration with
/// fixed-int encoding and a selectable byte order. ``REQ_0212``,
/// ``BB_0003``.
///
/// # Length contract
///
/// Fixed-width primitives encode to a constant number of bytes,
/// independent of the value:
///
/// | type                  | bytes |
/// |-----------------------|-------|
/// | `u8` / `i8`           | 1     |
/// | `u16` / `i16`         | 2     |
/// | `u32` / `i32` / `f32` | 4     |
/// | `u64` / `i64` / `f64` | 8     |
///
/// A struct of fixed-width fields encodes to the sum of its field widths
/// (no padding, no framing). This constant width is the reason the codec
/// exists: a routing slice's `bit_length` can be a static constant.
///
/// NOT constant-length: variable-length types still vary with their
/// contents. `String` / `&str` and `Vec<_>` are length-prefixed, and
/// enums carry a discriminant — do not assume a static wire width for
/// those.
///
/// Holds only an [`Endian`] selector (effectively zero-sized);
/// clone-cheap; thread-safe.
#[derive(Copy, Clone, Debug, Default)]
pub struct BinaryCodec {
    endian: Endian,
}

impl BinaryCodec {
    /// Static format name carried in [`ConnectorError::Codec`]. Constant
    /// `"binary"`.
    pub const FORMAT_NAME: &'static str = "binary";

    /// Construct a big-endian codec — network / EtherCAT-PDI byte order.
    /// Identical to [`Default`].
    #[must_use]
    pub const fn big_endian() -> Self {
        Self {
            endian: Endian::Big,
        }
    }

    /// Construct a little-endian codec.
    #[must_use]
    pub const fn little_endian() -> Self {
        Self {
            endian: Endian::Little,
        }
    }

    /// Construct a codec for an explicit [`Endian`].
    #[must_use]
    pub const fn new(endian: Endian) -> Self {
        Self { endian }
    }

    /// The byte order this codec encodes / decodes with.
    #[must_use]
    pub const fn endian(&self) -> Endian {
        self.endian
    }

    /// Encode `value` into `buf` under `config`. Shared by the
    /// endianness branches so each only supplies its `config` value.
    fn encode_with<T, C>(value: &T, buf: &mut [u8], config: C) -> Result<usize, ConnectorError>
    where
        T: serde::Serialize,
        C: Config,
    {
        let max = buf.len();
        match bincode::serde::encode_into_slice(value, buf, config) {
            Ok(written) => Ok(written),
            // The slice writer signals buffer exhaustion as `UnexpectedEnd`.
            // Re-encode to a Vec on this (failure-only) path to report the
            // size the payload would have needed; the success path stays
            // allocation-free.
            Err(bincode::error::EncodeError::UnexpectedEnd) => {
                let actual =
                    bincode::serde::encode_to_vec(value, config).map_or(max + 1, |v| v.len());
                Err(ConnectorError::PayloadOverflow { actual, max })
            }
            Err(e) => Err(ConnectorError::codec(Self::FORMAT_NAME, e)),
        }
    }

    /// Decode `T` from `buf` under `config`. Shared by the endianness
    /// branches.
    fn decode_with<T, C>(buf: &[u8], config: C) -> Result<T, ConnectorError>
    where
        T: serde::de::DeserializeOwned,
        C: Config,
    {
        bincode::serde::decode_from_slice::<T, C>(buf, config)
            .map(|(value, _read)| value)
            .map_err(|e| ConnectorError::codec(Self::FORMAT_NAME, e))
    }
}

impl PayloadCodec for BinaryCodec {
    fn format_name(&self) -> &'static str {
        Self::FORMAT_NAME
    }

    fn encode<T>(&self, value: &T, buf: &mut [u8]) -> Result<usize, ConnectorError>
    where
        T: serde::Serialize,
    {
        let base = bincode::config::standard().with_fixed_int_encoding();
        match self.endian {
            Endian::Big => Self::encode_with(value, buf, base.with_big_endian()),
            Endian::Little => Self::encode_with(value, buf, base.with_little_endian()),
        }
    }

    fn decode<T>(&self, buf: &[u8]) -> Result<T, ConnectorError>
    where
        T: serde::de::DeserializeOwned,
    {
        let base = bincode::config::standard().with_fixed_int_encoding();
        match self.endian {
            Endian::Big => Self::decode_with(buf, base.with_big_endian()),
            Endian::Little => Self::decode_with(buf, base.with_little_endian()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use serde::{Deserialize, Serialize};
    use taktora_connector_core::ConnectorError;

    /// A small fixed-width struct: 2 + 1 = 3 bytes, no padding or framing.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    struct Pair {
        a: u16,
        b: u8,
    }

    fn round_trip<T>(codec: BinaryCodec, value: &T) -> T
    where
        T: Serialize + serde::de::DeserializeOwned,
    {
        let mut buf = [0u8; 64];
        let n = codec.encode(value, &mut buf).expect("encode fits");
        codec.decode(&buf[..n]).expect("decode succeeds")
    }

    #[test]
    fn u16_encodes_to_exactly_two_bytes() {
        // The whole point of the codec: a fixed-width primitive is a
        // constant length regardless of value. A routing `bit_length`
        // depends on this being exactly 2 for a `u16`.
        let codec = BinaryCodec::big_endian();
        let mut buf = [0u8; 64];
        for v in [0u16, 1, 42, 255, 256, 0x1234, u16::MAX] {
            let n = codec.encode(&v, &mut buf).expect("encode fits");
            assert_eq!(n, 2, "u16 {v} must encode to exactly 2 bytes, got {n}");
        }
    }

    #[test]
    fn fixed_width_primitives_have_constant_length() {
        let codec = BinaryCodec::big_endian();
        let mut buf = [0u8; 64];
        assert_eq!(codec.encode(&0u8, &mut buf).unwrap(), 1);
        assert_eq!(codec.encode(&0u16, &mut buf).unwrap(), 2);
        assert_eq!(codec.encode(&0u32, &mut buf).unwrap(), 4);
        assert_eq!(codec.encode(&0u64, &mut buf).unwrap(), 8);
        assert_eq!(codec.encode(&0i8, &mut buf).unwrap(), 1);
        assert_eq!(codec.encode(&0i16, &mut buf).unwrap(), 2);
        assert_eq!(codec.encode(&0i32, &mut buf).unwrap(), 4);
        assert_eq!(codec.encode(&0i64, &mut buf).unwrap(), 8);
    }

    #[test]
    fn struct_encodes_to_summed_field_width() {
        let codec = BinaryCodec::big_endian();
        let mut buf = [0u8; 64];
        let n = codec
            .encode(&Pair { a: 0x1234, b: 0x56 }, &mut buf)
            .unwrap();
        assert_eq!(n, 3, "u16 + u8 must be 2 + 1 = 3 bytes");
        assert_eq!(&buf[..n], &[0x12, 0x34, 0x56]);
    }

    #[test]
    fn big_endian_byte_order_is_network_order() {
        let codec = BinaryCodec::big_endian();
        let mut buf = [0u8; 64];
        let n = codec.encode(&0x1234u16, &mut buf).unwrap();
        assert_eq!(&buf[..n], &[0x12, 0x34]);
    }

    #[test]
    fn little_endian_byte_order_is_reversed() {
        let codec = BinaryCodec::little_endian();
        let mut buf = [0u8; 64];
        let n = codec.encode(&0x1234u16, &mut buf).unwrap();
        assert_eq!(&buf[..n], &[0x34, 0x12]);
    }

    #[test]
    fn default_is_big_endian() {
        assert_eq!(BinaryCodec::default().endian(), Endian::Big);
        assert_eq!(BinaryCodec::big_endian().endian(), Endian::Big);
        assert_eq!(BinaryCodec::new(Endian::Little).endian(), Endian::Little);
    }

    #[test]
    fn format_name_is_binary() {
        assert_eq!(BinaryCodec::big_endian().format_name(), "binary");
    }

    #[test]
    fn round_trips_primitives_and_struct() {
        let codec = BinaryCodec::big_endian();
        assert_eq!(round_trip(codec, &7u8), 7u8);
        assert_eq!(round_trip(codec, &0xBEEFu16), 0xBEEFu16);
        assert_eq!(round_trip(codec, &0xDEAD_BEEFu32), 0xDEAD_BEEFu32);
        assert_eq!(round_trip(codec, &-12345i32), -12345i32);
        let pair = Pair { a: 4242, b: 7 };
        assert_eq!(round_trip(codec, &pair), pair);
    }

    #[test]
    fn encode_into_too_small_buffer_overflows() {
        let codec = BinaryCodec::big_endian();
        let mut buf = [0u8; 1]; // a u16 needs 2
        match codec.encode(&0x1234u16, &mut buf) {
            Err(ConnectorError::PayloadOverflow { actual, max }) => {
                assert_eq!(actual, 2);
                assert_eq!(max, 1);
            }
            other => panic!("expected PayloadOverflow, got {other:?}"),
        }
    }

    #[test]
    fn decode_of_truncated_input_is_codec_error() {
        let codec = BinaryCodec::big_endian();
        // One byte where a u16 (two bytes) is expected.
        let err = codec
            .decode::<u16>(&[0x12])
            .expect_err("truncated must fail");
        assert!(
            matches!(err, ConnectorError::Codec { .. }),
            "expected Codec error, got {err:?}"
        );
    }

    proptest! {
        #[test]
        fn u16_always_two_bytes(v in any::<u16>()) {
            let codec = BinaryCodec::big_endian();
            let mut buf = [0u8; 8];
            let n = codec.encode(&v, &mut buf).unwrap();
            prop_assert_eq!(n, 2);
        }

        #[test]
        fn round_trip_pair(a in any::<u16>(), b in any::<u8>()) {
            for codec in [BinaryCodec::big_endian(), BinaryCodec::little_endian()] {
                let pair = Pair { a, b };
                prop_assert_eq!(round_trip(codec, &pair), pair);
            }
        }
    }
}
