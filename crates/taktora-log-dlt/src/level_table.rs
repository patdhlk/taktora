//! Per-context log-level table backed by `AtomicU8`.
//!
//! REQ_0810: control messages from `dlt-daemon` update per-context
//! levels at runtime without locking.
//!
//! REQ_0811: production default is `INFO`.
//!
//! # DLT R20-11 LogLevel encoding
//!
//! The numeric constants in this module match the on-wire encoding
//! specified in AUTOSAR DLT R20-11:
//!
//! | Value | Name    |
//! |-------|---------|
//! | 0     | OFF     |
//! | 1     | FATAL   |
//! | 2     | ERROR   |
//! | 3     | WARN    |
//! | 4     | INFO    |
//! | 5     | DEBUG   |
//! | 6     | VERBOSE |
//!
//! The control-message parser (T15) decodes these bytes from the wire
//! and calls [`LevelTable::set`] directly.

use std::collections::HashMap;
use std::sync::RwLock;
use std::sync::atomic::{AtomicU8, Ordering};

use crate::ids::CtxId;

/// DLT R20-11: logging disabled entirely.
// Used by T15 control-message parser and internal tests; allow dead_code
// until T15 is wired in.
#[allow(dead_code)]
const OFF: u8 = 0;
/// DLT R20-11: fatal error — mapped to [`log::Level::Error`].
// Used by T15 control-message parser and internal tests; allow dead_code
// until T15 is wired in.
#[allow(dead_code)]
const FATAL: u8 = 1;
/// DLT R20-11: error condition.
const ERROR: u8 = 2;
/// DLT R20-11: warning condition.
const WARN: u8 = 3;
/// DLT R20-11: informational message (production default per REQ_0811).
const INFO: u8 = 4;
/// DLT R20-11: debug-level detail.
const DEBUG: u8 = 5;
/// DLT R20-11: verbose/trace-level detail — mapped to [`log::Level::Trace`].
const VERBOSE: u8 = 6;

/// Per-context, lock-free-read log-level table.
///
/// Each registered [`CtxId`] gets its own [`AtomicU8`] slot so that
/// `dlt-daemon` control messages can update individual context levels
/// concurrently (REQ_0810).  A single atomic `default` covers all
/// contexts not yet mentioned by a control message.
///
/// ## Concurrency model
///
/// * [`current`][LevelTable::current] and [`set`][LevelTable::set] on
///   an already-known context acquire only a **read** lock on the map.
/// * [`set`][LevelTable::set] on an unknown context promotes to a
///   **write** lock once to insert the new slot, then immediately
///   stores the requested value.  This slow path fires at most once per
///   context for the lifetime of the table.
/// * [`set_default`][LevelTable::set_default] touches only the global
///   `default` atomic — no map lock at all.
#[derive(Debug)]
pub struct LevelTable {
    /// Fallback level for contexts that have no individual entry.
    default: AtomicU8,
    /// Per-context level slots, keyed by [`CtxId`].
    by_ctx: RwLock<HashMap<CtxId, AtomicU8>>,
}

impl LevelTable {
    /// Create a new table with `default` as the initial level for every context.
    pub fn new(default: log::Level) -> Self {
        Self {
            default: AtomicU8::new(level_to_u8(default)),
            by_ctx: RwLock::new(HashMap::new()),
        }
    }

    /// Change the fallback level used by contexts that have no individual entry.
    ///
    /// This is an atomic store; concurrent [`current`][Self::current] calls
    /// on unknown contexts will observe the new value immediately.
    pub fn set_default(&self, l: log::Level) {
        self.default.store(level_to_u8(l), Ordering::Release);
    }

    /// Set the active log level for a specific context.
    ///
    /// If the context already has a slot in the map the update is done
    /// under a **read** lock only (atomic store into the existing
    /// [`AtomicU8`]).  If the context is new the call briefly takes a
    /// **write** lock to insert the slot, then stores the value.
    pub fn set(&self, ctx: &CtxId, l: log::Level) {
        // Fast path: context already registered — read lock only.
        if let Some(slot) = self.by_ctx.read().unwrap().get(ctx) {
            slot.store(level_to_u8(l), Ordering::Release);
            return;
        }
        // Slow path: first time we see this context — promote to write lock.
        let mut w = self.by_ctx.write().unwrap();
        w.entry(ctx.clone())
            .or_insert_with(|| AtomicU8::new(0))
            .store(level_to_u8(l), Ordering::Release);
    }

    /// Return the current active log level for `ctx`.
    ///
    /// If `ctx` has been registered via [`set`][Self::set] its individual
    /// level is returned; otherwise the global default is used.
    pub fn current(&self, ctx: &CtxId) -> log::Level {
        if let Some(slot) = self.by_ctx.read().unwrap().get(ctx) {
            u8_to_level(slot.load(Ordering::Acquire))
        } else {
            u8_to_level(self.default.load(Ordering::Acquire))
        }
    }
}

/// Translate a [`log::Level`] to its DLT R20-11 wire byte.
fn level_to_u8(l: log::Level) -> u8 {
    match l {
        log::Level::Error => ERROR,
        log::Level::Warn => WARN,
        log::Level::Info => INFO,
        log::Level::Debug => DEBUG,
        log::Level::Trace => VERBOSE,
    }
}

/// Translate a DLT R20-11 wire byte to the closest [`log::Level`].
///
/// FATAL (1) is folded into Error because the `log` crate has no Fatal
/// variant.  OFF (0) and any unrecognised byte are mapped to Error so
/// that unknown bytes never silently enable excess logging.
fn u8_to_level(v: u8) -> log::Level {
    if v == WARN {
        log::Level::Warn
    } else if v == INFO {
        log::Level::Info
    } else if v == DEBUG {
        log::Level::Debug
    } else if v == VERBOSE {
        log::Level::Trace
    } else {
        // OFF, FATAL, ERROR, and any unrecognised byte → Error.
        log::Level::Error
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_roundtrip_all_variants() {
        for &level in &[
            log::Level::Error,
            log::Level::Warn,
            log::Level::Info,
            log::Level::Debug,
            log::Level::Trace,
        ] {
            assert_eq!(u8_to_level(level_to_u8(level)), level);
        }
    }

    #[test]
    fn fatal_and_off_map_to_error() {
        assert_eq!(u8_to_level(OFF), log::Level::Error);
        assert_eq!(u8_to_level(FATAL), log::Level::Error);
    }

    #[test]
    fn unknown_byte_maps_to_error() {
        assert_eq!(u8_to_level(0xFF), log::Level::Error);
    }
}
