//! Encode `log::Record` values as `dlt_core::dlt::Message` and
//! serialise to bytes. REQ_0806.

use std::sync::atomic::{AtomicU8, Ordering};

use dlt_core::dlt::{
    Argument, DltTimeStamp, Endianness, ExtendedHeaderConfig, FloatWidth, LogLevel, Message,
    MessageConfig, MessageType, PayloadContent, StorageHeader, StringCoding, TypeInfo,
    TypeInfoKind, TypeLength, Value,
};

use crate::ids::{AppId, CtxId};
use crate::kv;

/// Encodes `log::Record`s as serialised DLT bytes.
pub struct Encoder {
    app: AppId,
    default_ctx: CtxId,
    ecu_id: String,
    counter: AtomicU8,
}

impl Encoder {
    /// Build a new encoder using `app` / `default_ctx` for every record
    /// (the per-target context override lands in Task 14).
    pub fn new(app: AppId, default_ctx: CtxId, ecu_id: String) -> Self {
        Self {
            app,
            default_ctx,
            ecu_id,
            counter: AtomicU8::new(0),
        }
    }

    /// Encode `record` into its serialised DLT byte form.
    ///
    /// `timestamp_tenths_ms` is the DLT standard-header timestamp
    /// (1/10 ms ticks since some reference instant; the daemon expects
    /// monotonically increasing values per ECU).
    pub fn encode(&self, record: &log::Record<'_>, timestamp_tenths_ms: u32) -> Vec<u8> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let storage = StorageHeader {
            timestamp: DltTimeStamp {
                seconds: now.as_secs() as u32,
                microseconds: now.subsec_micros(),
            },
            ecu_id: self.ecu_id.clone(),
        };

        // Build verbose arguments: leading formatted message + one
        // verbose arg per log::kv pair (handled in `kv` module).
        let mut args: Vec<Argument> = Vec::with_capacity(1 + 4);
        args.push(string_argument(&format!("{}", record.args())));
        kv::collect_arguments(record, &mut args);

        let extended_info = ExtendedHeaderConfig {
            message_type: MessageType::Log(map_level(record.level())),
            app_id: self.app.as_str().to_string(),
            context_id: self.default_ctx.as_str().to_string(),
        };

        let conf = MessageConfig {
            version: 1,
            counter: self.counter.fetch_add(1, Ordering::Relaxed),
            endianness: Endianness::Big,
            ecu_id: Some(self.ecu_id.clone()),
            session_id: None,
            timestamp: Some(timestamp_tenths_ms),
            payload: PayloadContent::Verbose(args),
            extended_header_info: Some(extended_info),
        };

        // `Message::new` computes the correct `payload_length` from the
        // already-built payload, so we don't have to mirror dlt-core's
        // serialiser arithmetic here.
        let message = Message::new(conf, Some(storage));

        message.as_bytes()
    }
}

fn map_level(l: log::Level) -> LogLevel {
    match l {
        log::Level::Error => LogLevel::Error,
        log::Level::Warn => LogLevel::Warn,
        log::Level::Info => LogLevel::Info,
        log::Level::Debug => LogLevel::Debug,
        log::Level::Trace => LogLevel::Verbose,
    }
}

fn string_argument(s: &str) -> Argument {
    Argument {
        type_info: TypeInfo {
            kind: TypeInfoKind::StringType,
            coding: StringCoding::UTF8,
            has_variable_info: false,
            has_trace_info: false,
        },
        name: None,
        unit: None,
        fixed_point: None,
        value: Value::StringVal(s.to_string()),
    }
}

#[allow(dead_code)] // consumed by `kv.rs` in Task 11
pub(crate) fn unsigned_argument(v: u32) -> Argument {
    Argument {
        type_info: TypeInfo {
            kind: TypeInfoKind::Unsigned(TypeLength::BitLength32),
            coding: StringCoding::UTF8,
            has_variable_info: false,
            has_trace_info: false,
        },
        name: None,
        unit: None,
        fixed_point: None,
        value: Value::U32(v),
    }
}

#[allow(dead_code)] // consumed by `kv.rs` in Task 11
pub(crate) fn signed_argument(v: i32) -> Argument {
    Argument {
        type_info: TypeInfo {
            kind: TypeInfoKind::Signed(TypeLength::BitLength32),
            coding: StringCoding::UTF8,
            has_variable_info: false,
            has_trace_info: false,
        },
        name: None,
        unit: None,
        fixed_point: None,
        value: Value::I32(v),
    }
}

#[allow(dead_code)] // consumed by `kv.rs` in Task 11
pub(crate) fn float_argument(v: f64) -> Argument {
    Argument {
        type_info: TypeInfo {
            kind: TypeInfoKind::Float(FloatWidth::Width64),
            coding: StringCoding::UTF8,
            has_variable_info: false,
            has_trace_info: false,
        },
        name: None,
        unit: None,
        fixed_point: None,
        value: Value::F64(v),
    }
}

#[allow(dead_code)] // consumed by `kv.rs` in Task 11
pub(crate) fn bool_argument(v: bool) -> Argument {
    Argument {
        type_info: TypeInfo {
            kind: TypeInfoKind::Bool,
            coding: StringCoding::UTF8,
            has_variable_info: false,
            has_trace_info: false,
        },
        name: None,
        unit: None,
        fixed_point: None,
        value: Value::Bool(v as u8),
    }
}

#[allow(dead_code)] // consumed by `kv.rs` in Task 11
pub(crate) fn display_string_argument(s: &str) -> Argument {
    string_argument(s)
}
