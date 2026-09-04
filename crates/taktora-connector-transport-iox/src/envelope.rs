//! [`ConnectorEnvelope`] — POD wire format with CRC-32 integrity checking
//! used by every connector channel. `REQ_0200`, `REQ_0202`, `REQ_0203`, `REQ_0204`, `TSR_0008`.

use iceoryx2::prelude::ZeroCopySend;

/// 32-byte correlation id carried end-to-end (`REQ_0204`). The framework
/// does not interpret these bytes — application layers may.
pub type CorrelationId = [u8; 32];

/// On-wire envelope: a fixed POD header followed by an inline payload
/// buffer of compile-time size `N`. Wire version 2 adds CRC-32 integrity
/// checking (`TSR_0008`).
///
/// `#[repr(C)]` + `ZeroCopySend` make this safe to publish via
/// iceoryx2's loan path. Every field is plain-old-data; the struct is
/// `Copy` for convenience in tests, but production sends use
/// [`iceoryx2::port::publisher::Publisher::loan_uninit`] to avoid the
/// `Copy`-induced stack/SHM round-trip (`REQ_0205`).
///
/// ## Memory layout (64-byte header)
///
/// All fields are naturally aligned on 64-bit targets:
///
/// - `sequence_number: u64` @ 0
/// - `timestamp_ns: u64` @ 8
/// - `correlation_id: [u8; 32]` @ 16
/// - `payload_len: u32` @ 48
/// - `reserved: u32` @ 52
/// - `crc32: u32` @ 56
/// - `version: u16` @ 60
/// - `padding: u16` @ 62
/// - `payload: [u8; N]` @ 64
///
/// Total header: 64 bytes. The trailing `[u8; N]` starts at offset 64
/// (aligned to 1 byte, no padding regardless of `N`).
#[repr(C)]
#[derive(Clone, Copy, Debug, ZeroCopySend)]
pub struct ConnectorEnvelope<const N: usize> {
    /// Per-(publisher, channel) strictly monotonically increasing
    /// counter starting at zero. `REQ_0202`.
    pub sequence_number: u64,
    /// Nanoseconds since the UNIX epoch at the moment the envelope was
    /// loaned for send. `REQ_0203`.
    pub timestamp_ns: u64,
    /// Application-controlled correlation id. The framework carries
    /// these bytes verbatim (`REQ_0204`); senders that do not need
    /// correlation should leave this zeroed.
    pub correlation_id: CorrelationId,
    /// Number of valid bytes in [`Self::payload`]. Always `<= N`.
    /// Receivers must trust this value (the framework validates it
    /// against `N` at send time — `TEST_0125`).
    pub payload_len: u32,
    /// Caller-defined metadata slot. Defaults to zero, and legacy
    /// senders (`send_raw_bytes`, [`Default`]) always write zero.
    /// Senders MAY stamp a non-zero value via `send_raw_bytes_v2` (or
    /// any future v2+ writer) to carry caller-defined metadata;
    /// receivers MUST NOT assume zero. The connector-zenoh layer is the
    /// only documented user today, where this field carries a per-call
    /// query timeout (`REQ_0425`).
    pub reserved: u32,
    /// CRC-32 (IEEE polynomial) over the header fields (excluding this
    /// field itself) plus the valid payload bytes (`payload[..payload_len]`).
    /// Computed on send, verified on receive; mismatch drops the frame
    /// and raises a health event (`TSR_0008`).
    pub crc32: u32,
    /// Wire format version. Version 2 introduced CRC integrity checking.
    /// Future wire changes increment this field; readers can version-gate
    /// features or reject unsupported envelopes.
    pub version: u16,
    /// Explicit padding to keep the header at 64 bytes with natural
    /// alignment. Must be zeroed on send; receivers ignore this field.
    pub padding: u16,
    /// Inline payload buffer. Only the first `payload_len` bytes are
    /// valid; the rest is uninitialised (the loan path may leave
    /// previously-sent bytes in the tail).
    pub payload: [u8; N],
}

impl<const N: usize> Default for ConnectorEnvelope<N> {
    fn default() -> Self {
        Self {
            sequence_number: 0,
            timestamp_ns: 0,
            correlation_id: [0u8; 32],
            payload_len: 0,
            reserved: 0,
            crc32: 0,
            version: Self::WIRE_VERSION,
            padding: 0,
            payload: [0u8; N],
        }
    }
}

impl<const N: usize> ConnectorEnvelope<N> {
    /// Current wire format version. Version 2 adds CRC-32 integrity checking.
    pub const WIRE_VERSION: u16 = 2;

    /// Compute the CRC-32 (IEEE polynomial) over this envelope's header
    /// (excluding the `crc32` field itself) plus the valid payload bytes.
    ///
    /// The CRC covers:
    /// - `sequence_number` (8 bytes @ offset 0)
    /// - `timestamp_ns` (8 bytes @ offset 8)
    /// - `correlation_id` (32 bytes @ offset 16)
    /// - `payload_len` (4 bytes @ offset 48)
    /// - `reserved` (4 bytes @ offset 52)
    /// - `version` (2 bytes @ offset 60)
    /// - `padding` (2 bytes @ offset 62)
    /// - `payload[..payload_len]` (variable length)
    ///
    /// The `crc32` field itself (4 bytes @ offset 56) is **excluded** from
    /// the checksum.
    ///
    /// # Examples
    ///
    /// ```
    /// use taktora_connector_transport_iox::ConnectorEnvelope;
    ///
    /// let mut env = ConnectorEnvelope::<128>::default();
    /// env.payload_len = 4;
    /// env.payload[..4].copy_from_slice(b"test");
    /// let crc = env.compute_crc();
    /// env.crc32 = crc;
    /// assert!(env.verify_crc());
    /// ```
    #[must_use]
    pub fn compute_crc(&self) -> u32 {
        let mut hasher = crc32fast::Hasher::new();

        // Hash header fields before crc32 (offsets 0..56)
        hasher.update(&self.sequence_number.to_ne_bytes());
        hasher.update(&self.timestamp_ns.to_ne_bytes());
        hasher.update(&self.correlation_id);
        hasher.update(&self.payload_len.to_ne_bytes());
        hasher.update(&self.reserved.to_ne_bytes());

        // Skip crc32 field itself (offset 56..60)

        // Hash header fields after crc32 (offsets 60..64)
        hasher.update(&self.version.to_ne_bytes());
        hasher.update(&self.padding.to_ne_bytes());

        // Hash valid payload bytes
        let payload_len = (self.payload_len as usize).min(N);
        hasher.update(&self.payload[..payload_len]);

        hasher.finalize()
    }

    /// Verify that this envelope's `crc32` field matches the computed
    /// checksum over the header and payload.
    ///
    /// Returns `true` if the CRC is valid (envelope integrity confirmed),
    /// `false` otherwise (corruption detected).
    ///
    /// # Examples
    ///
    /// ```
    /// use taktora_connector_transport_iox::ConnectorEnvelope;
    ///
    /// let mut env = ConnectorEnvelope::<128>::default();
    /// env.payload_len = 4;
    /// env.payload[..4].copy_from_slice(b"test");
    /// env.crc32 = env.compute_crc();
    /// assert!(env.verify_crc());
    ///
    /// // Corrupt a payload byte
    /// env.payload[0] ^= 0xFF;
    /// assert!(!env.verify_crc());
    /// ```
    #[must_use]
    pub fn verify_crc(&self) -> bool {
        self.crc32 == self.compute_crc()
    }

    /// Maximum bytes the payload buffer can carry. Equals the `N` const
    /// generic; exposed as a function so callers don't have to remember
    /// the bound's name.
    #[must_use]
    pub const fn capacity() -> usize {
        N
    }

    /// Borrow the valid prefix of the payload buffer. The returned slice
    /// has length [`Self::payload_len`].
    #[must_use]
    pub fn payload_bytes(&self) -> &[u8] {
        let len = (self.payload_len as usize).min(N);
        &self.payload[..len]
    }
}
