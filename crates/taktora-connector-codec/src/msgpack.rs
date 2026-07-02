//! [`MsgPackCodec`] — `rmp-serde`-backed `MessagePack` `PayloadCodec`.
//! ``REQ_0989``, ``BB_0003``.
//!
//! Encode writes directly into the caller-provided buffer (via a small
//! `CountingWriter` adapter), so a successful encode does not allocate on
//! the heap. Buffer-too-small surfaces as
//! [`ConnectorError::PayloadOverflow`] for consistency with
//! `ChannelWriter::send`'s overflow behaviour (`TEST_0125`); other
//! serializer failures surface as [`ConnectorError::Codec`] (`REQ_0213`).
//!
//! Decode delegates to [`rmp_serde::from_slice`]; failures (truncated
//! input, schema mismatch) surface as [`ConnectorError::Codec`]
//! (`REQ_0214`) rather than being silently dropped.
//!
//! # Wire format
//!
//! Uses `rmp-serde`'s default (compact) encoding: structs serialize as
//! `MessagePack` **arrays** (positional fields), not maps keyed by field
//! name. The wire form is therefore *not* self-describing — encoder and
//! decoder must share the Rust type, exactly as with [`crate::BinaryCodec`].
//! This yields smaller payloads than JSON without the fixed-width
//! constant-length contract `BinaryCodec` provides; `MessagePack` integers
//! are still variable-length, so do not assume a static wire width.

use std::io;

use taktora_connector_core::{ConnectorError, PayloadCodec};

/// `MessagePack` codec built on `rmp-serde`. Zero-sized; clone-cheap;
/// thread-safe. ``REQ_0989``, ``BB_0003``.
#[derive(Copy, Clone, Debug, Default)]
pub struct MsgPackCodec;

impl MsgPackCodec {
    /// Static format name carried in [`ConnectorError::Codec`]. Constant
    /// `"msgpack"`.
    pub const FORMAT_NAME: &'static str = "msgpack";

    /// Construct a fresh codec. `Default` is identical; provided as a
    /// convenience for explicit construction in `static` contexts.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl PayloadCodec for MsgPackCodec {
    fn format_name(&self) -> &'static str {
        Self::FORMAT_NAME
    }

    fn encode<T>(&self, value: &T, buf: &mut [u8]) -> Result<usize, ConnectorError>
    where
        T: serde::Serialize,
    {
        let max = buf.len();
        let mut writer = CountingWriter::new(buf);
        match rmp_serde::encode::write(&mut writer, value) {
            Ok(()) => Ok(writer.bytes_written()),
            Err(e) => {
                // rmp-serde wraps the `CountingWriter`'s buffer-exhaustion
                // `io::Error` deep inside its error enum, and the exact
                // shape is version-sensitive. Rather than match on it, re-
                // encode to a Vec on this (failure-only) path: if that
                // succeeds and overflows `max`, the original failure was
                // buffer exhaustion; otherwise it was a genuine serializer
                // fault. The success path above stays allocation-free.
                match rmp_serde::to_vec(value) {
                    Ok(bytes) if bytes.len() > max => Err(ConnectorError::PayloadOverflow {
                        actual: bytes.len(),
                        max,
                    }),
                    _ => Err(ConnectorError::codec(Self::FORMAT_NAME, e)),
                }
            }
        }
    }

    fn decode<T>(&self, buf: &[u8]) -> Result<T, ConnectorError>
    where
        T: serde::de::DeserializeOwned,
    {
        rmp_serde::from_slice(buf).map_err(|e| ConnectorError::codec(Self::FORMAT_NAME, e))
    }
}

/// Counts bytes written and returns [`io::ErrorKind::WriteZero`] on
/// overflow so [`rmp_serde::encode::write`] surfaces a recognisable
/// failure. Holds a `&mut [u8]` borrow — caller owns the buffer.
///
/// A near-identical adapter lives in the `json` module; the two are kept
/// separate so each codec module compiles independently under its own
/// cargo feature.
struct CountingWriter<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a> CountingWriter<'a> {
    const fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    const fn bytes_written(&self) -> usize {
        self.pos
    }
}

impl io::Write for CountingWriter<'_> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let remaining = self.buf.len() - self.pos;
        if data.len() > remaining {
            return Err(io::Error::new(io::ErrorKind::WriteZero, "buffer full"));
        }
        self.buf[self.pos..self.pos + data.len()].copy_from_slice(data);
        self.pos += data.len();
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use serde::{Deserialize, Serialize};
    use taktora_connector_core::ConnectorError;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct Sample {
        id: u32,
        label: String,
        flags: Vec<bool>,
    }

    fn round_trip<T>(value: &T) -> T
    where
        T: Serialize + serde::de::DeserializeOwned,
    {
        let codec = MsgPackCodec::new();
        let mut buf = [0u8; 256];
        let n = codec.encode(value, &mut buf).expect("encode fits");
        codec.decode(&buf[..n]).expect("decode succeeds")
    }

    #[test]
    fn format_name_is_msgpack() {
        assert_eq!(MsgPackCodec::new().format_name(), "msgpack");
    }

    #[test]
    fn round_trips_primitives_and_struct() {
        assert_eq!(round_trip(&7u8), 7u8);
        assert_eq!(round_trip(&0xDEAD_BEEFu32), 0xDEAD_BEEFu32);
        assert_eq!(round_trip(&-12345i32), -12345i32);
        assert_eq!(round_trip(&"hello".to_string()), "hello".to_string());
        let s = Sample {
            id: 42,
            label: "sensor".to_string(),
            flags: vec![true, false, true],
        };
        assert_eq!(round_trip(&s), s);
    }

    #[test]
    fn encoding_is_more_compact_than_json() {
        // Not a hard guarantee of the format, but a sanity check that the
        // `MessagePack` encoding is doing its job on a representative struct.
        let s = Sample {
            id: 42,
            label: "sensor".to_string(),
            flags: vec![true, false, true],
        };
        let codec = MsgPackCodec::new();
        let mut buf = [0u8; 256];
        let n = codec.encode(&s, &mut buf).expect("encode fits");
        let json_len = serde_json::to_vec(&s).unwrap().len();
        assert!(
            n < json_len,
            "msgpack ({n}) should be smaller than json ({json_len})"
        );
    }

    #[test]
    fn encode_into_too_small_buffer_overflows() {
        let codec = MsgPackCodec::new();
        let value = "a string that will not fit".to_string();
        let mut buf = [0u8; 4];
        match codec.encode(&value, &mut buf) {
            Err(ConnectorError::PayloadOverflow { actual, max }) => {
                assert!(actual > max, "actual {actual} should exceed max {max}");
                assert_eq!(max, 4);
            }
            other => panic!("expected PayloadOverflow, got {other:?}"),
        }
    }

    #[test]
    fn decode_of_truncated_input_is_codec_error() {
        let codec = MsgPackCodec::new();
        // Encode a struct, then lop off the tail so decode must fail.
        let s = Sample {
            id: 1,
            label: "x".to_string(),
            flags: vec![true],
        };
        let mut buf = [0u8; 256];
        let n = codec.encode(&s, &mut buf).expect("encode fits");
        let err = codec
            .decode::<Sample>(&buf[..n - 1])
            .expect_err("truncated must fail");
        assert!(
            matches!(err, ConnectorError::Codec { .. }),
            "expected Codec error, got {err:?}"
        );
    }

    proptest! {
        #[test]
        fn round_trip_sample(id in any::<u32>(), label in ".{0,32}", flags in prop::collection::vec(any::<bool>(), 0..8)) {
            let s = Sample { id, label, flags };
            prop_assert_eq!(round_trip(&s), s);
        }
    }
}
