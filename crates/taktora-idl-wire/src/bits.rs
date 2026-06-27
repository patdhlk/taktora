//! CAN signal bit-packing, addressing both DBC bit-numbering conventions
//! (`REQ_0862`).
//!
//! A DBC signal is described by a *start bit*, a *bit length*, and a byte
//! order. The two byte orders number bits differently within the frame:
//!
//! * **Little-endian (Intel, `@1`)** — the start bit is the signal's LSB, and
//!   the signal occupies consecutive LSB0-absolute bit positions
//!   `start ..= start + len - 1`.
//! * **Big-endian (Motorola, `@0`)** — the start bit is the signal's MSB, and
//!   successive (less significant) bits walk *down* within a byte, jumping to
//!   bit 7 of the next byte on underflow (the "sawtooth" layout).
//!
//! Both are reduced here to the same primitive: a mapping from each *value bit*
//! to an absolute LSB0 position in the frame, where absolute position `p`
//! addresses bit `p % 8` of byte `p / 8` (bit 0 = least significant). Encode
//! and decode share that mapping, so the two directions cannot drift apart.

use crate::WireError;

/// Bit-numbering convention of a CAN signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteOrder {
    /// Intel / `@1`: start bit is the LSB; positions ascend.
    LittleEndian,
    /// Motorola / `@0`: start bit is the MSB; sawtooth descent.
    BigEndian,
}

const MAX_BITS: u16 = 64;

/// Invoke `f(value_bit, abs_pos)` for every bit of a `bit_len`-wide signal,
/// where `value_bit` is the bit index within the raw value (0 = LSB) and
/// `abs_pos` is its absolute LSB0 position in the frame.
///
/// Returns the maximum absolute position touched, for a bounds pre-check.
fn for_each_bit(
    start_bit: u16,
    bit_len: u16,
    order: ByteOrder,
    mut f: impl FnMut(u16, usize),
) -> usize {
    let mut max_pos = 0usize;
    match order {
        ByteOrder::LittleEndian => {
            for i in 0..bit_len {
                let abs = usize::from(start_bit) + usize::from(i);
                max_pos = max_pos.max(abs);
                f(i, abs);
            }
        }
        ByteOrder::BigEndian => {
            // k counts from the MSB (k = 0) down; value bit = len - 1 - k.
            let mut byte = usize::from(start_bit / 8);
            let mut bit = usize::from(start_bit % 8); // 0..=7, 0 = LSB
            for k in 0..bit_len {
                let abs = byte * 8 + bit;
                max_pos = max_pos.max(abs);
                f(bit_len - 1 - k, abs);
                if bit == 0 {
                    bit = 7;
                    byte += 1;
                } else {
                    bit -= 1;
                }
            }
        }
    }
    max_pos
}

/// Validate the bit length and that the whole signal fits inside `frame_len`
/// bytes, returning the byte span needed.
fn check(
    frame_len: usize,
    start_bit: u16,
    bit_len: u16,
    order: ByteOrder,
) -> Result<(), WireError> {
    if bit_len == 0 || bit_len > MAX_BITS {
        return Err(WireError::InvalidBitLength);
    }
    let max_pos = for_each_bit(start_bit, bit_len, order, |_, _| {});
    if max_pos / 8 >= frame_len {
        return Err(WireError::SignalOutOfBounds);
    }
    Ok(())
}

/// Pack the low `bit_len` bits of `raw` into `frame` at the signal's position.
///
/// Each addressed bit is written explicitly (cleared then set), so the result
/// does not depend on `frame`'s prior contents at those positions.
///
/// # Errors
///
/// [`WireError::InvalidBitLength`] for `bit_len` outside `1..=64`, or
/// [`WireError::SignalOutOfBounds`] if the signal does not fit in `frame`.
pub fn pack_unsigned(
    frame: &mut [u8],
    start_bit: u16,
    bit_len: u16,
    order: ByteOrder,
    raw: u64,
) -> Result<(), WireError> {
    check(frame.len(), start_bit, bit_len, order)?;
    for_each_bit(start_bit, bit_len, order, |value_bit, abs| {
        let byte = abs / 8;
        let mask = 1u8 << (abs % 8);
        if (raw >> value_bit) & 1 == 1 {
            frame[byte] |= mask;
        } else {
            frame[byte] &= !mask;
        }
    });
    Ok(())
}

/// Pack a signed `value` (two's-complement, truncated to `bit_len` bits).
///
/// # Errors
///
/// As [`pack_unsigned`].
#[allow(clippy::cast_sign_loss)] // reinterpret two's-complement bits, by design
pub fn pack_signed(
    frame: &mut [u8],
    start_bit: u16,
    bit_len: u16,
    order: ByteOrder,
    value: i64,
) -> Result<(), WireError> {
    let raw = (value as u64) & mask(bit_len);
    pack_unsigned(frame, start_bit, bit_len, order, raw)
}

/// Extract the signal's raw bits from `frame` as an unsigned value.
///
/// # Errors
///
/// As [`pack_unsigned`].
pub fn unpack_unsigned(
    frame: &[u8],
    start_bit: u16,
    bit_len: u16,
    order: ByteOrder,
) -> Result<u64, WireError> {
    check(frame.len(), start_bit, bit_len, order)?;
    let mut raw = 0u64;
    for_each_bit(start_bit, bit_len, order, |value_bit, abs| {
        let bit = (frame[abs / 8] >> (abs % 8)) & 1;
        raw |= u64::from(bit) << value_bit;
    });
    Ok(raw)
}

/// Extract the signal's raw bits and sign-extend from `bit_len` to `i64`.
///
/// # Errors
///
/// As [`pack_unsigned`].
pub fn unpack_signed(
    frame: &[u8],
    start_bit: u16,
    bit_len: u16,
    order: ByteOrder,
) -> Result<i64, WireError> {
    let raw = unpack_unsigned(frame, start_bit, bit_len, order)?;
    Ok(sign_extend(raw, bit_len))
}

/// A `bit_len`-wide low-bit mask (`bit_len == 64` yields all ones).
const fn mask(bit_len: u16) -> u64 {
    if bit_len >= 64 {
        u64::MAX
    } else {
        (1u64 << bit_len) - 1
    }
}

/// Sign-extend the low `bit_len` bits of `raw` into a full `i64`.
#[allow(clippy::cast_possible_wrap)] // two's-complement reinterpretation, by design
const fn sign_extend(raw: u64, bit_len: u16) -> i64 {
    if bit_len >= 64 {
        return raw as i64;
    }
    let shift = 64 - bit_len;
    // Shift the sign bit to the top and back down with an arithmetic shift.
    ((raw << shift) as i64) >> shift
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn little_endian_round_trip_byte_aligned() {
        let mut frame = [0u8; 8];
        pack_unsigned(&mut frame, 0, 16, ByteOrder::LittleEndian, 0x1234).unwrap();
        // LSB at byte 0.
        assert_eq!(frame[0], 0x34);
        assert_eq!(frame[1], 0x12);
        let back = unpack_unsigned(&frame, 0, 16, ByteOrder::LittleEndian).unwrap();
        assert_eq!(back, 0x1234);
    }

    #[test]
    fn little_endian_sub_byte_signal() {
        let mut frame = [0u8; 2];
        // 4-bit signal starting at bit 4 of byte 0.
        pack_unsigned(&mut frame, 4, 4, ByteOrder::LittleEndian, 0xA).unwrap();
        assert_eq!(frame[0], 0xA0);
        let back = unpack_unsigned(&frame, 4, 4, ByteOrder::LittleEndian).unwrap();
        assert_eq!(back, 0xA);
    }

    #[test]
    fn big_endian_msb_at_start_bit() {
        // 16-bit Motorola signal with MSB at bit 7 (byte 0) — the classic
        // big-endian layout puts the high byte first.
        let mut frame = [0u8; 8];
        pack_unsigned(&mut frame, 7, 16, ByteOrder::BigEndian, 0x1234).unwrap();
        assert_eq!(frame[0], 0x12);
        assert_eq!(frame[1], 0x34);
        let back = unpack_unsigned(&frame, 7, 16, ByteOrder::BigEndian).unwrap();
        assert_eq!(back, 0x1234);
    }

    #[test]
    fn signed_round_trip_and_sign_extension() {
        let mut frame = [0u8; 8];
        pack_signed(&mut frame, 16, 8, ByteOrder::LittleEndian, -40).unwrap();
        let back = unpack_signed(&frame, 16, 8, ByteOrder::LittleEndian).unwrap();
        assert_eq!(back, -40);
        // Positive value too.
        pack_signed(&mut frame, 0, 12, ByteOrder::LittleEndian, 1000).unwrap();
        assert_eq!(
            unpack_signed(&frame, 0, 12, ByteOrder::LittleEndian).unwrap(),
            1000
        );
    }

    #[test]
    fn pack_clears_stale_bits() {
        let mut frame = [0xFFu8; 2];
        pack_unsigned(&mut frame, 0, 4, ByteOrder::LittleEndian, 0x5).unwrap();
        assert_eq!(frame[0] & 0x0F, 0x5);
    }

    #[test]
    fn out_of_bounds_and_bad_length_are_rejected() {
        let mut frame = [0u8; 1];
        assert_eq!(
            pack_unsigned(&mut frame, 4, 8, ByteOrder::LittleEndian, 0),
            Err(WireError::SignalOutOfBounds)
        );
        assert_eq!(
            pack_unsigned(&mut frame, 0, 0, ByteOrder::LittleEndian, 0),
            Err(WireError::InvalidBitLength)
        );
        assert_eq!(
            pack_unsigned(&mut frame, 0, 65, ByteOrder::LittleEndian, 0),
            Err(WireError::InvalidBitLength)
        );
    }

    #[test]
    fn full_width_64_bit() {
        let mut frame = [0u8; 8];
        let v = 0xDEAD_BEEF_CAFE_F00D;
        pack_unsigned(&mut frame, 0, 64, ByteOrder::LittleEndian, v).unwrap();
        assert_eq!(
            unpack_unsigned(&frame, 0, 64, ByteOrder::LittleEndian).unwrap(),
            v
        );
        assert_eq!(mask(64), u64::MAX);
    }
}
