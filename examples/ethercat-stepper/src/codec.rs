//! Fixed-size byte-image passthrough codec for the connector channel payload.

use taktora_connector_core::{ConnectorError, PayloadCodec};

/// Passthrough codec for a fixed-size byte image. The connector channel
/// payload type is `[u8; LEN]`; this copies bytes verbatim on the wire.
#[derive(Debug, Clone, Copy, Default)]
pub struct RawImageCodec;

impl PayloadCodec for RawImageCodec {
    fn format_name(&self) -> &'static str {
        "raw-image"
    }

    /// # Errors
    ///
    /// Returns [`ConnectorError::Codec`] if `value` does not serialise to a
    /// byte array, and [`ConnectorError::PayloadOverflow`] if `buf` is
    /// smaller than the encoded image.
    fn encode<T>(&self, value: &T, buf: &mut [u8]) -> Result<usize, ConnectorError>
    where
        T: serde::Serialize,
    {
        let v = serde_json::to_value(value).map_err(|e| ConnectorError::codec("raw-image", e))?;
        let arr = v.as_array().ok_or_else(|| {
            ConnectorError::codec("raw-image", std::io::Error::other("expected byte array"))
        })?;
        if buf.len() < arr.len() {
            return Err(ConnectorError::PayloadOverflow {
                actual: arr.len(),
                max: buf.len(),
            });
        }
        for (slot, n) in buf.iter_mut().zip(arr.iter()) {
            *slot = u8::try_from(n.as_u64().ok_or_else(|| {
                ConnectorError::codec("raw-image", std::io::Error::other("non-integer byte"))
            })?)
            .map_err(|_| ConnectorError::codec("raw-image", std::io::Error::other("byte > 255")))?;
        }
        Ok(arr.len())
    }

    /// # Errors
    ///
    /// Returns [`ConnectorError::Codec`] if the bytes do not deserialise
    /// into `T` (e.g. wrong array length for a fixed-size image).
    fn decode<T>(&self, buf: &[u8]) -> Result<T, ConnectorError>
    where
        T: serde::de::DeserializeOwned,
    {
        let arr: Vec<serde_json::Value> = buf
            .iter()
            .map(|b| serde_json::Value::Number((*b).into()))
            .collect();
        serde_json::from_value(serde_json::Value::Array(arr))
            .map_err(|e| ConnectorError::codec("raw-image", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_22_byte_image() {
        let src: [u8; 22] = core::array::from_fn(|i| i as u8);
        let mut buf = [0u8; 22];
        let n = RawImageCodec.encode(&src, &mut buf).unwrap();
        assert_eq!(n, 22);
        let out: [u8; 22] = RawImageCodec.decode(&buf[..n]).unwrap();
        assert_eq!(out, src);
    }

    #[test]
    fn encode_errors_when_buffer_too_small() {
        let src = [0u8; 22];
        let mut buf = [0u8; 8];
        assert!(RawImageCodec.encode(&src, &mut buf).is_err());
    }
}
