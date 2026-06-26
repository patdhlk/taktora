//! [`BoundedString`] — an inline, fixed-capacity, `Copy` UTF-8 string used as a
//! POD ViewModel/command field.
//!
//! The image written into the seqlock cell must be `#[repr(C)]` and `Copy`, so
//! it cannot hold a heap `String`. `BoundedString<CAP>` stores a `u16` length
//! plus an inline `[u8; CAP]` buffer, mirroring the `str<cap>` contract field
//! type (`len: u16` + `[u8; cap]`). It serializes as a normal UTF-8 JSON string
//! so the cross-language contract sees an ordinary string.

use std::fmt;

use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize, Serializer};

/// An inline, fixed-capacity (`CAP` bytes), `Copy` UTF-8 string.
///
/// The stored bytes `[0, len)` are always valid UTF-8. Construction truncates
/// on a UTF-8 character boundary so the value never exceeds `CAP` bytes; `CAP`
/// must not exceed `u16::MAX`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct BoundedString<const CAP: usize> {
    len: u16,
    bytes: [u8; CAP],
}

impl<const CAP: usize> BoundedString<CAP> {
    /// An empty `BoundedString`.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            len: 0,
            bytes: [0u8; CAP],
        }
    }

    /// The byte capacity.
    #[must_use]
    pub const fn capacity() -> usize {
        CAP
    }

    /// The current byte length.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len as usize
    }

    /// Whether the string is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The string contents.
    #[must_use]
    pub fn as_str(&self) -> &str {
        // SAFETY-free: the bytes in `[0, len)` are maintained as valid UTF-8 by
        // every constructor, so this never fails. The end is clamped to `CAP` so
        // a `len > CAP` value (possible from a future raw-byte / torn seqlock
        // copy) cannot index past the buffer and panic.
        let end = self.len().min(CAP);
        std::str::from_utf8(&self.bytes[..end]).unwrap_or("")
    }

    /// Build from a `&str`, truncating on a UTF-8 boundary to fit `CAP`.
    #[must_use]
    pub fn from_str_truncating(s: &str) -> Self {
        debug_assert!(
            CAP <= u16::MAX as usize,
            "BoundedString CAP exceeds u16::MAX"
        );
        let cap = CAP.min(u16::MAX as usize);
        // Largest prefix length that fits in `cap` and lands on a char boundary.
        let mut end = s.len().min(cap);
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        let mut bytes = [0u8; CAP];
        bytes[..end].copy_from_slice(&s.as_bytes()[..end]);
        Self {
            len: end as u16,
            bytes,
        }
    }

    /// Construct directly from raw fields, bypassing every invariant.
    ///
    /// Test-only: lets a test build a deliberately malformed value (e.g. with
    /// `len > CAP`) to prove `as_str` clamps instead of panicking. No production
    /// code path can produce such a value safely.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn from_raw_parts(len: u16, bytes: [u8; CAP]) -> Self {
        Self { len, bytes }
    }
}

impl<const CAP: usize> Default for BoundedString<CAP> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const CAP: usize> From<&str> for BoundedString<CAP> {
    fn from(s: &str) -> Self {
        Self::from_str_truncating(s)
    }
}

impl<const CAP: usize> fmt::Debug for BoundedString<CAP> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

impl<const CAP: usize> fmt::Display for BoundedString<CAP> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl<const CAP: usize> PartialEq for BoundedString<CAP> {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl<const CAP: usize> Eq for BoundedString<CAP> {}

impl<const CAP: usize> Serialize for BoundedString<CAP> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de, const CAP: usize> Deserialize<'de> for BoundedString<CAP> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct StrVisitor<const CAP: usize>;
        impl<const CAP: usize> Visitor<'_> for StrVisitor<CAP> {
            type Value = BoundedString<CAP>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "a string of at most {CAP} UTF-8 bytes")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(BoundedString::from_str_truncating(v))
            }
        }
        deserializer.deserialize_str(StrVisitor::<CAP>)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_and_reads_back() {
        let s = BoundedString::<16>::from("hello");
        assert_eq!(s.as_str(), "hello");
        assert_eq!(s.len(), 5);
        assert!(!s.is_empty());
    }

    #[test]
    fn default_is_empty() {
        let s = BoundedString::<8>::default();
        assert!(s.is_empty());
        assert_eq!(s.as_str(), "");
    }

    #[test]
    fn truncates_on_char_boundary() {
        // "é" is two UTF-8 bytes; with CAP=3 only one fits cleanly.
        let s = BoundedString::<3>::from("ééé");
        assert_eq!(s.as_str(), "é");
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn is_copy_and_pod_sized() {
        let s = BoundedString::<16>::from("x");
        let c = s; // Copy, not move
        assert_eq!(s, c);
        // len: u16 (2) + [u8;16] = 18, padded to 18 (align 2).
        assert_eq!(std::mem::size_of::<BoundedString<16>>(), 18);
    }

    #[test]
    fn as_str_does_not_panic_on_out_of_range_len() {
        // Simulate a torn / raw-byte copy that left `len` larger than `CAP`.
        let bytes = *b"hello\0\0\0";
        let s = BoundedString::<8>::from_raw_parts(99, bytes);
        // Must not panic; the end is clamped to `CAP`, so we get the valid
        // UTF-8 prefix of the buffer back (the trailing NULs are valid UTF-8).
        let out = s.as_str();
        assert_eq!(out.len(), 8);
        assert!(out.starts_with("hello"));
    }

    #[test]
    fn as_str_returns_empty_on_invalid_utf8_with_bad_len() {
        // Non-UTF-8 bytes plus an out-of-range len: still no panic, empty value.
        let bytes = [0xFFu8; 8];
        let s = BoundedString::<8>::from_raw_parts(255, bytes);
        assert_eq!(s.as_str(), "");
    }

    #[test]
    fn serializes_as_a_plain_string() {
        let s = BoundedString::<16>::from("jog");
        assert_eq!(serde_json::to_string(&s).unwrap(), "\"jog\"");
        let back: BoundedString<16> = serde_json::from_str("\"jog\"").unwrap();
        assert_eq!(back, s);
    }
}
