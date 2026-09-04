//! Runtime trait contract for generated `EtherCAT` device drivers.
//!
//! This crate is the minimal, dependency-light runtime contract that
//! generated device structs implement and that downstream consumers link
//! against. It deliberately does **not** depend on `ethercrab`.

pub use taktora_fieldbus_od_core::Identity;

// Re-export the `bitvec` types that generated code and consumers share, so
// they all agree on a single bit ordering and slice element type.
pub use bitvec::{order::Lsb0, slice::BitSlice};

/// Object-safe runtime trait every generated `EtherCAT` device implements.
///
/// Object safety is a hard requirement: `dyn EsiDevice` must compile so that
/// heterogeneous devices can be held behind a trait object. That is why the
/// identity is exposed as a method rather than an associated const.
pub trait EsiDevice {
    /// The device's vendor / product / revision identity triple.
    fn identity(&self) -> Identity;

    /// Process-image input size in **bytes** (byte-rounded).
    fn input_len(&self) -> usize;

    /// Process-image output size in **bytes** (byte-rounded).
    fn output_len(&self) -> usize;

    /// Decode the input process image into the device's typed state.
    fn decode_inputs(&mut self, bits: &BitSlice<u8, Lsb0>) -> Result<(), EsiError>;

    /// Encode the device's typed state into the output process image.
    fn encode_outputs(&self, bits: &mut BitSlice<u8, Lsb0>) -> Result<(), EsiError>;
}

/// Runtime decode/encode error for an [`EsiDevice`].
///
/// This is distinct from the parse-time `EsiError` in `taktora-ethercat-esi`:
/// that one reports failures while parsing ESI XML, whereas this one reports
/// failures while moving a live process image in and out of a device.
#[derive(Debug, thiserror::Error)]
pub enum EsiError {
    /// The supplied process-image slice was smaller than the device requires.
    #[error("process image too short: need {expected_bits} bits, got {got_bits}")]
    BufferTooShort {
        /// Number of bits the device needs.
        expected_bits: usize,
        /// Number of bits actually supplied.
        got_bits: usize,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitvec::field::BitField;
    use bitvec::view::BitView;

    /// Compile-level assertion that `EsiDevice` is object-safe.
    fn _assert_object_safe(_: &dyn EsiDevice) {}

    /// A tiny device: a 1-bit flag followed by an `i16`, laid out across the
    /// 3 input bytes of its process image. No outputs.
    #[derive(Default)]
    struct DummyDevice {
        flag: bool,
        value: i16,
    }

    impl DummyDevice {
        const INPUT_BITS: usize = 1 + 16;
    }

    impl EsiDevice for DummyDevice {
        fn identity(&self) -> Identity {
            Identity {
                vendor_id: 0x0000_1234,
                product_code: 0x0000_5678,
                revision: 0x0000_0001,
            }
        }

        fn input_len(&self) -> usize {
            // 17 bits, byte-rounded up to 3 bytes.
            Self::INPUT_BITS.div_ceil(8)
        }

        fn output_len(&self) -> usize {
            0
        }

        fn decode_inputs(&mut self, bits: &BitSlice<u8, Lsb0>) -> Result<(), EsiError> {
            if bits.len() < Self::INPUT_BITS {
                return Err(EsiError::BufferTooShort {
                    expected_bits: Self::INPUT_BITS,
                    got_bits: bits.len(),
                });
            }
            self.flag = bits[0];
            self.value = bits[1..17].load_le::<i16>();
            Ok(())
        }

        fn encode_outputs(&self, _bits: &mut BitSlice<u8, Lsb0>) -> Result<(), EsiError> {
            Ok(())
        }
    }

    /// `TEST_0430` — hand-written `EsiDevice` impl: identity, byte lengths and a
    /// `BitSlice<u8, Lsb0>` decode round-trip (`REQ_0530`, `REQ_0534`).
    #[test]
    fn decodes_known_process_image() {
        // bit 0 = flag (1), bits 1..17 = i16.
        // Build i16 = -2 (0xFFFE) placed at bit offset 1.
        // Layout: byte0 bit0 = flag, byte0 bits1..8 + byte1 + byte2 bit0 = value.
        let mut raw = [0u8; 3];
        {
            let bits = raw.view_bits_mut::<Lsb0>();
            bits.set(0, true); // flag
            bits[1..17].store_le::<u16>(0xFFFEu16); // i16 = -2
        }

        let mut dev = DummyDevice::default();
        let bits = raw.view_bits::<Lsb0>();
        dev.decode_inputs(bits).expect("decode should succeed");

        assert!(dev.flag);
        assert_eq!(dev.value, -2);
        assert_eq!(dev.input_len(), 3);
        assert_eq!(dev.output_len(), 0);
        assert_eq!(
            dev.identity(),
            Identity {
                vendor_id: 0x0000_1234,
                product_code: 0x0000_5678,
                revision: 0x0000_0001,
            }
        );
    }

    #[test]
    fn decode_rejects_short_buffer() {
        let raw = [0u8; 2]; // only 16 bits, need 17
        let mut dev = DummyDevice::default();
        let bits = raw.view_bits::<Lsb0>();

        let err = dev
            .decode_inputs(bits)
            .expect_err("should reject short buffer");
        match err {
            EsiError::BufferTooShort {
                expected_bits,
                got_bits,
            } => {
                assert_eq!(expected_bits, 17);
                assert_eq!(got_bits, 16);
            }
        }
    }
}
