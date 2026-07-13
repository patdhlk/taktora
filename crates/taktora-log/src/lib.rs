//! Workspace-wide [`log`]-crate facade for the taktora workspace.
//!
//! See the crate-level README and `spec/requirements/logging.rst` in
//! the taktora repository for the full specification.

// `forbid` rather than the workspace's usual `deny`: this is a pure-safe
// facade crate — every backend lives behind the `LogSink` trait, so there
// is no path that legitimately needs `unsafe`.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod sink;
pub use sink::LogSink;

mod adapter;
pub use adapter::LogSinkLogger;

pub mod console;

mod init;
pub use init::{Builder, InitError, init};

pub use log;
pub use log::{Level, LevelFilter, Record};
pub use log::{debug, error, info, trace, warn};
