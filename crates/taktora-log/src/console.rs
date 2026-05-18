//! Console dev-fallback backend used when no DLT daemon is configured
//! and no other `log::Log` implementation has been registered.
//!
//! See REQ_0816 / FEAT_0077 / ADR_0091 in `spec/`.

use std::io::{self, Write};
use std::sync::Mutex;

use log::{LevelFilter, Record};

use crate::LogSink;

/// Writes one human-readable line per record to a configurable sink
/// (defaults to stderr).
///
/// # Output format
///
/// `LEVEL  target  message  k=v k=v ...`
///
/// where structured key-value pairs from `log::kv` are rendered via
/// their `Display` form, separated by spaces.
pub struct Console<W: Write + Send> {
    writer: Mutex<W>,
    level: LevelFilter,
}

impl<W: Write + Send> Console<W> {
    /// Build a [`Console`] writing to `writer` with the given filter.
    pub fn with_writer(writer: W, level: LevelFilter) -> Self {
        Self {
            writer: Mutex::new(writer),
            level,
        }
    }
}

impl Console<io::Stderr> {
    /// The standard dev fallback — stderr at `LevelFilter::Info`.
    pub fn stderr_default() -> Self {
        Self::with_writer(io::stderr(), LevelFilter::Info)
    }
}

impl<W: Write + Send> LogSink for Console<W> {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= self.level
    }

    fn emit(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let mut buf = format!(
            "{level:<5}  {target}  {msg}",
            level = level_label(record.level()),
            target = record.target(),
            msg = record.args(),
        );

        // Append structured key-value pairs via Display.
        struct KvWriter<'a>(&'a mut String);
        impl<'kvs> log::kv::VisitSource<'kvs> for KvWriter<'_> {
            fn visit_pair(
                &mut self,
                key: log::kv::Key<'kvs>,
                value: log::kv::Value<'kvs>,
            ) -> Result<(), log::kv::Error> {
                use std::fmt::Write as _;
                let _ = write!(self.0, " {key}={value}");
                Ok(())
            }
        }
        let _ = record.key_values().visit(&mut KvWriter(&mut buf));
        buf.push('\n');

        if let Ok(mut w) = self.writer.lock() {
            let _ = w.write_all(buf.as_bytes());
        }
    }

    fn flush(&self) {
        if let Ok(mut w) = self.writer.lock() {
            let _ = w.flush();
        }
    }
}

fn level_label(l: log::Level) -> &'static str {
    match l {
        log::Level::Error => "ERROR",
        log::Level::Warn => "WARN",
        log::Level::Info => "INFO",
        log::Level::Debug => "DEBUG",
        log::Level::Trace => "TRACE",
    }
}
