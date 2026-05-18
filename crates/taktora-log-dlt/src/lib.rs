//! Pure-Rust AUTOSAR DLT backend for [`taktora-log`].
//!
//! See the crate-level README and `spec/requirements/logging.rst` in
//! the taktora repository for the full specification.

#![doc(html_root_url = "https://docs.rs/taktora-log-dlt/0.1.0")]
// `forbid` rather than the workspace's usual `deny`: this crate emits
// every byte via the safe `dlt-core` encoder and std sockets — there
// is no path that legitimately needs `unsafe`.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod encode;
pub mod ids;
mod kv;
pub mod level_table;
pub mod ring;
pub mod transport;
