//! Control-message ingest from `dlt-daemon`. REQ_0810.
//!
//! AUTOSAR Classic DLT defines several control services. For taktora
//! v1 we only handle the two that integrators actually drive at
//! runtime:
//!
//! * `Set-Log-Level` (service 0x01) — set per-Context level.
//! * `Set-Default-Log-Level` (service 0x11) — set the default applied
//!   to unknown contexts.
//!
//! Unsupported control messages are ignored silently.

use crate::ids::CtxId;
use crate::level_table::LevelTable;

/// A parsed control message ready to apply.
#[derive(Debug, Clone)]
pub enum ControlMessage {
    /// Set log level for a specific context (DLT service 0x01).
    SetLogLevel {
        /// Target context identifier.
        ctx: CtxId,
        /// Requested log level.
        level: log::Level,
    },
    /// Set the global default log level (DLT service 0x11).
    SetDefaultLogLevel(log::Level),
    /// Anything taktora-log-dlt does not handle — kept so the flusher
    /// can log it at DEBUG and move on.
    Unsupported {
        /// The unrecognised DLT control service identifier.
        service_id: u32,
    },
}

impl ControlMessage {
    /// Apply this message to `table`.
    pub fn apply(&self, table: &LevelTable) {
        match self {
            ControlMessage::SetLogLevel { ctx, level } => table.set(ctx, *level),
            ControlMessage::SetDefaultLogLevel(level) => table.set_default(*level),
            ControlMessage::Unsupported { .. } => {}
        }
    }
}

/// Parse a control message from raw payload bytes received from
/// `dlt-daemon`.
///
/// Wire format (host-side endian on the connection; AUTOSAR DLT
/// control payload):
/// * 4 bytes — service id (little-endian u32)
/// * For `Set-Log-Level` (0x01): 4 bytes AppId + 4 bytes CtxId
///   + 1 byte level + 4 bytes "ComId" (ignored).
/// * For `Set-Default-Log-Level` (0x11): 1 byte level
///   + 4 bytes "ComId" (ignored).
///
/// Returns `Unsupported { service_id }` for anything else.
pub fn parse_control(buf: &[u8]) -> Option<ControlMessage> {
    if buf.len() < 4 {
        return None;
    }
    let svc = u32::from_le_bytes(buf[0..4].try_into().ok()?);
    match svc {
        0x01 => {
            if buf.len() < 4 + 4 + 4 + 1 {
                return None;
            }
            // App id at [4..8] — not used beyond audit; the active app is
            // fixed per-encoder. Context id at [8..12].
            // Pass raw 4-byte slice to CtxId::new without trimming: the
            // daemon pads short IDs with \0, and CtxId accepts any 4 ASCII
            // bytes including \0.
            let ctx_str = std::str::from_utf8(&buf[8..12]).ok()?;
            let ctx = CtxId::new(ctx_str).ok()?;
            let level = decode_dlt_level(buf[12])?;
            Some(ControlMessage::SetLogLevel { ctx, level })
        }
        0x11 => {
            if buf.len() < 4 + 1 {
                return None;
            }
            let level = decode_dlt_level(buf[4])?;
            Some(ControlMessage::SetDefaultLogLevel(level))
        }
        other => Some(ControlMessage::Unsupported { service_id: other }),
    }
}

fn decode_dlt_level(b: u8) -> Option<log::Level> {
    match b {
        1 | 2 => Some(log::Level::Error), // FATAL collapses to Error
        3 => Some(log::Level::Warn),
        4 => Some(log::Level::Info),
        5 => Some(log::Level::Debug),
        6 => Some(log::Level::Trace),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a Set-Log-Level (0x01) payload for the given app/ctx ascii
    /// id bytes and DLT level byte.
    fn set_log_level_buf(app: &[u8; 4], ctx: &[u8; 4], level: u8) -> Vec<u8> {
        let mut buf = vec![0x01, 0x00, 0x00, 0x00]; // service id LE
        buf.extend_from_slice(app);
        buf.extend_from_slice(ctx);
        buf.push(level);
        buf.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // ComId (ignored)
        buf
    }

    /// Build a Set-Default-Log-Level (0x11) payload.
    fn set_default_buf(level: u8) -> Vec<u8> {
        let mut buf = vec![0x11, 0x00, 0x00, 0x00]; // service id LE
        buf.push(level);
        buf.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // ComId (ignored)
        buf
    }

    #[test]
    fn parse_set_log_level_happy_path() {
        let buf = set_log_level_buf(b"MYAP", b"MAIN", 5 /* Debug */);
        let msg = parse_control(&buf).expect("parse should succeed");
        match msg {
            ControlMessage::SetLogLevel { ctx, level } => {
                assert_eq!(ctx.as_str(), "MAIN");
                assert_eq!(level, log::Level::Debug);
            }
            other => panic!("expected SetLogLevel, got {:?}", other),
        }
    }

    #[test]
    fn parse_set_default_log_level_happy_path() {
        let buf = set_default_buf(3 /* Warn */);
        let msg = parse_control(&buf).expect("parse should succeed");
        match msg {
            ControlMessage::SetDefaultLogLevel(level) => {
                assert_eq!(level, log::Level::Warn);
            }
            other => panic!("expected SetDefaultLogLevel, got {:?}", other),
        }
    }

    #[test]
    fn parse_too_short_buffer_returns_none() {
        // 3 bytes — not enough for the 4-byte service-id field.
        assert!(parse_control(&[0x01, 0x00, 0x00]).is_none());
    }

    #[test]
    fn parse_set_log_level_truncated_returns_none() {
        // Has a valid service-id but the payload is too short (missing ctx + level).
        assert!(parse_control(&[0x01, 0x00, 0x00, 0x00, 0x41, 0x42]).is_none());
    }

    #[test]
    fn parse_unknown_service_returns_unsupported() {
        // Service 0xBEEF is not handled by taktora-log-dlt.
        let buf = [0xEF, 0xBE, 0x00, 0x00];
        let msg = parse_control(&buf).expect("should return Unsupported, not None");
        match msg {
            ControlMessage::Unsupported { service_id } => {
                assert_eq!(service_id, 0xBEEF);
            }
            other => panic!("expected Unsupported, got {:?}", other),
        }
    }

    #[test]
    fn parse_set_log_level_null_padded_ctx_id() {
        // Daemon sends a 2-char context "TX" padded with two \0 bytes.
        let buf = set_log_level_buf(b"MYAP", b"TX\x00\x00", 4 /* Info */);
        let msg = parse_control(&buf).expect("parse should succeed with null-padded ctx");
        match msg {
            ControlMessage::SetLogLevel { ctx, level } => {
                assert_eq!(ctx.as_str(), "TX\x00\x00");
                assert_eq!(level, log::Level::Info);
            }
            other => panic!("expected SetLogLevel, got {:?}", other),
        }
    }
}
