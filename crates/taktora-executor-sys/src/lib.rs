//! # taktora-executor-sys
//!
//! The audited unsafe boundary of [`taktora-executor`](https://docs.rs/taktora-executor).
//! `taktora-executor` is `#![forbid(unsafe_code)]`; every `unsafe` block it
//! needs lives here, behind a safe API whose soundness argument is documented
//! at the definition site (`// SAFETY:` on every block).
//!
//! * [`os`] — Linux FFI: `timerfd`, thread timer slack, `SCHED_FIFO`.
//! * [`ports`] — `Send` wrappers for iceoryx2 `ipc::Service` ports.
//! * [`dispatch`] — borrowed-job and dispatch-table pointers used by the
//!   executor's `WaitSet` thread, worker pool, and graph runner.
//!
//! Not intended for direct use outside `taktora-executor`.

pub mod dispatch;
pub mod os;
pub mod ports;
