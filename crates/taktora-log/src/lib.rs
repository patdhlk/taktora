//! Workspace-wide [`log`]-crate facade for the taktora workspace.
//!
//! See the crate-level README and `spec/requirements/logging.rst` in
//! the taktora repository for the full specification.

#![doc(html_root_url = "https://docs.rs/taktora-log/0.1.0")]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub use log;
pub use log::{Level, LevelFilter, Record};
pub use log::{debug, error, info, trace, warn};
