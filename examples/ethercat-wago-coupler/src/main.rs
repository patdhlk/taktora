//! Integration example: executor + ethercat connector against a real
//! WAGO 750-354 EtherCAT coupler carrying a 750-430 (8 digital inputs)
//! and a 750-530 (8 digital outputs), over a real Linux NIC. See
//! README.md for hardware setup and run instructions.
//!
//! Topology — the key contrast with the Beckhoff `ethercat-real-bus`
//! example. The WAGO 750-354 coupler is the ONLY EtherCAT SubDevice on
//! the bus. The 750-430, 750-530, and 750-600 are internal K-bus
//! modules whose I/O is aggregated into the coupler's single process
//! image. `ethercrab`'s `init_single_group` assigns the coupler the
//! configured station address `0x1000`. The 8 input bits (750-430) and
//! 8 output bits (750-530) are separate Tx and Rx slices on that one
//! SubDevice, both at bit offset 0 because they live in distinct
//! process images.
//!
//! Behaviour: each 10 ms scan cycle the example reads the 750-430's 8
//! input bits and writes them straight through to the 750-530's 8
//! output bits (a digital input-to-output mirror), printing on change.

use taktora_connector_core::{ConnectorError, PayloadCodec};

/// One-byte codec used by this example. `JsonCodec` can't be used here
/// because the WAGO process image is raw bits, not JSON text. This
/// codec round-trips a `u8` to/from a single byte on the wire, matching
/// the 8-bit digital slices on the 750-430 and 750-530.
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

fn main() {
    // Replaced in Task 2 with the real executor + connector wiring.
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
