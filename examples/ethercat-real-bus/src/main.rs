//! Integration example: executor + ethercat connector against an
//! EK1100 + EL1008 over a real Linux NIC. See README.md for hardware
//! setup and run instructions.

use taktora_connector_core::{ConnectorError, PayloadCodec};

/// One-byte codec used by this example. `JsonCodec` can't be used
/// here because the EL1008's PDI is raw bits, not JSON text. This
/// codec round-trips a `u8` to/from a single byte on the wire,
/// matching the EL1008's 8-bit Tx PDO layout.
// `RawByteCodec` is exercised by the unit tests and will be plugged
// into `main()` in Task 3. Suppress dead-code until then.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, Default)]
struct RawByteCodec;

impl PayloadCodec for RawByteCodec {
    fn format_name(&self) -> &'static str {
        "raw-byte"
    }

    /// # Errors
    ///
    /// Returns [`ConnectorError::Codec`] if `value` does not serialise
    /// to a `u64`-shaped integer in `0..=255`, and
    /// [`ConnectorError::PayloadOverflow`] if `buf` is empty.
    fn encode<T>(&self, value: &T, buf: &mut [u8]) -> Result<usize, ConnectorError>
    where
        T: serde::Serialize,
    {
        let v = serde_json::to_value(value).map_err(|e| ConnectorError::codec("raw-byte", e))?;
        let byte: u8 = v
            .as_u64()
            .ok_or_else(|| {
                ConnectorError::codec(
                    "raw-byte",
                    std::io::Error::other("expected u8-like integer"),
                )
            })?
            .try_into()
            .map_err(|_| {
                ConnectorError::codec(
                    "raw-byte",
                    std::io::Error::other("value does not fit in u8"),
                )
            })?;
        if buf.is_empty() {
            return Err(ConnectorError::PayloadOverflow { actual: 1, max: 0 });
        }
        buf[0] = byte;
        Ok(1)
    }

    /// # Errors
    ///
    /// Returns [`ConnectorError::Codec`] if `buf` is empty or if the
    /// single byte does not deserialise into `T`.
    fn decode<T>(&self, buf: &[u8]) -> Result<T, ConnectorError>
    where
        T: serde::de::DeserializeOwned,
    {
        if buf.is_empty() {
            return Err(ConnectorError::codec(
                "raw-byte",
                std::io::Error::other("empty buffer; expected exactly 1 byte"),
            ));
        }
        let byte = buf[0];
        let v = serde_json::Value::Number(serde_json::Number::from(byte));
        serde_json::from_value(v).map_err(|e| ConnectorError::codec("raw-byte", e))
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("ethercat-real-bus stub — implementation lands in Task 3");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_zero() {
        let mut buf = [0_u8; 1];
        let n = RawByteCodec.encode(&0_u8, &mut buf).unwrap();
        assert_eq!(n, 1);
        assert_eq!(buf[0], 0);
        let v: u8 = RawByteCodec.decode(&buf[..n]).unwrap();
        assert_eq!(v, 0);
    }

    #[test]
    fn round_trips_max_byte() {
        let mut buf = [0_u8; 1];
        let n = RawByteCodec.encode(&255_u8, &mut buf).unwrap();
        assert_eq!(n, 1);
        assert_eq!(buf[0], 255);
        let v: u8 = RawByteCodec.decode(&buf[..n]).unwrap();
        assert_eq!(v, 255);
    }

    #[test]
    fn decode_empty_buf_errors() {
        let result: Result<u8, _> = RawByteCodec.decode(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn encode_empty_buf_errors() {
        let mut buf: [u8; 0] = [];
        let result = RawByteCodec.encode(&42_u8, &mut buf);
        assert!(result.is_err());
    }
}
