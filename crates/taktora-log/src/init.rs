//! One-shot init builder.
//!
//! See REQ_0803 (settable exactly once), REQ_0804 (pre-existing logger
//! wins), REQ_0805 (tracing-log bridge installed at init), and REQ_0816
//! (console fallback when no sink supplied and nothing else installed).

use std::sync::{Arc, OnceLock};

use log::{LevelFilter, SetLoggerError};
use thiserror::Error;

use crate::{LogSink, adapter::LogSinkLogger, console::Console};

static INSTALLED: OnceLock<Arc<dyn LogSink>> = OnceLock::new();

/// Errors returned by [`Builder::start`].
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum InitError {
    /// `taktora_log::init()` has already been called in this process.
    #[error("taktora-log has already been initialized")]
    AlreadyInitialized,

    /// Another `log::Log` implementation was installed before `init()`.
    /// Per REQ_0804 we do not override it.
    #[error("a log::Log implementation is already installed; not overriding")]
    PreExistingLogger,
}

/// Builder for [`init()`].
pub struct Builder {
    sink: Option<Arc<dyn LogSink>>,
    max_level: LevelFilter,
    install_tracing_bridge: bool,
}

impl Builder {
    /// Use `sink` as the active backend.
    pub fn with_sink(mut self, sink: Arc<dyn LogSink>) -> Self {
        self.sink = Some(sink);
        self
    }

    /// Set the global max level filter applied by the `log` crate
    /// before records reach the sink. Defaults to `LevelFilter::Info`
    /// (REQ_0811).
    pub fn with_max_level(mut self, level: LevelFilter) -> Self {
        self.max_level = level;
        self
    }

    /// Skip installing the `tracing-log` bridge. Default is `true`.
    pub fn with_tracing_bridge(mut self, enable: bool) -> Self {
        self.install_tracing_bridge = enable;
        self
    }

    /// Install the configured sink as the global `log::Log` and (by
    /// default) install the `tracing-log` bridge.
    ///
    /// Returns an [`InitError`] if another logger is already installed
    /// or `init` has been called in this process before.
    pub fn start(self) -> Result<(), InitError> {
        if INSTALLED.get().is_some() {
            return Err(InitError::AlreadyInitialized);
        }
        let sink = self
            .sink
            .unwrap_or_else(|| Arc::new(Console::stderr_default()) as Arc<dyn LogSink>);

        // Install as global. `set_boxed_logger` returns `Err` if any
        // logger is already installed — per REQ_0804 we surface that
        // as `PreExistingLogger`.
        let logger = Box::new(LogSinkLogger::new(Arc::clone(&sink)));
        if let Err(SetLoggerError { .. }) = log::set_boxed_logger(logger) {
            return Err(InitError::PreExistingLogger);
        }
        log::set_max_level(self.max_level);

        if self.install_tracing_bridge {
            // tracing-log's LogTracer captures tracing::Events as
            // log::Records. Install only if not already installed; the
            // `init_with_filter` helper internally guards against double-init.
            let _ = tracing_log::LogTracer::init_with_filter(self.max_level);
        }

        // Mark installed *after* `set_boxed_logger` succeeded so a
        // second init() detects the AlreadyInitialized case. We can't
        // use `.expect()` on the returned `Result<_, Arc<dyn LogSink>>`
        // because `dyn LogSink` doesn't implement `Debug`. The map_err
        // collapses the impossible second-init path (already rejected
        // above) into a unit.
        if INSTALLED.set(sink).is_err() {
            // Unreachable under normal use: `INSTALLED.get().is_some()`
            // was checked at the top of `start`, and `log::set_boxed_logger`
            // serializes the global. A racing thread could have set both
            // since then, in which case the global is now installed and
            // we report that the other init won.
            return Err(InitError::AlreadyInitialized);
        }
        Ok(())
    }
}

/// Entry point for configuring and installing the taktora-log facade.
///
/// ```no_run
/// use std::sync::Arc;
/// use taktora_log::{init, console::Console, LogSink};
///
/// init()
///     .with_sink(Arc::new(Console::stderr_default()) as Arc<dyn LogSink>)
///     .start()
///     .expect("first init in this process");
/// ```
pub fn init() -> Builder {
    Builder {
        sink: None,
        max_level: LevelFilter::Info,
        install_tracing_bridge: true,
    }
}
