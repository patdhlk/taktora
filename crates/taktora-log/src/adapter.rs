//! Adapter that exposes any `LogSink` as a `log::Log`.

use std::sync::Arc;

use log::{Log, Metadata, Record};

use crate::LogSink;

/// A `log::Log` implementation that forwards every call to a
/// [`LogSink`]. Used by the facade's builder to install a backend
/// as the global logger.
pub struct LogSinkLogger {
    sink: Arc<dyn LogSink>,
}

impl LogSinkLogger {
    /// Build a logger wrapping `sink`.
    pub fn new(sink: Arc<dyn LogSink>) -> Self {
        Self { sink }
    }
}

impl Log for LogSinkLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        self.sink.enabled(metadata)
    }
    fn log(&self, record: &Record<'_>) {
        if self.sink.enabled(record.metadata()) {
            self.sink.emit(record);
        }
    }
    fn flush(&self) {
        self.sink.flush();
    }
}
