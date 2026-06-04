//! Off-RT-thread telemetry export for `taktora-executor`.
//!
//! The executor pushes one [`CycleObservation`][obs] per task per cycle via
//! [`Observer::on_cycle_stats`][cb] — **synchronously on the RT `WaitSet`
//! thread**. This crate moves those samples off that thread without blocking
//! it, for offline analysis (the executor jitter envelope, `REQ_0110` /
//! `REQ_0111`):
//!
//! * [`NdjsonRingObserver`] — the producer side: flattens each observation
//!   into a [`PodRecord`] and pushes it into a [`CycleRing`]. Never does I/O.
//! * [`CycleRing`] — a single-producer / single-consumer **overwrite-oldest**
//!   ring with per-slot sequence numbers (a seqlock). The producer never
//!   blocks; a lagging consumer is *lapped* and the loss is counted, never
//!   silently swallowed.
//! * [`spawn`](writer::spawn) — the consumer side: a drain thread that pops
//!   records and writes NDJSON to a sink, reporting a [`DrainSummary`].
//!
//! # Seqlock soundness
//!
//! The consumer may read a slot's bytes while the producer overwrites them;
//! the read is validated against the slot's sequence number afterward and
//! discarded if torn. The ring slot stores [`PodRecord`] — integers and a
//! presence bitmask only, **no enums** — so a torn read is always a valid bit
//! pattern (no invalid enum discriminant) and, because `PodRecord: Copy`,
//! never triggers `Drop`. The residual strict-aliasing caveat of the racy
//! read is inherent to the seqlock pattern; a fully MIRI-clean per-word-atomic
//! variant is deferred to a future production exporter.
//!
//! [obs]: taktora_executor::CycleObservation
//! [cb]: taktora_executor::Observer::on_cycle_stats

mod observer;
mod record;
mod ring;
pub mod writer;

pub use observer::NdjsonRingObserver;
pub use record::PodRecord;
pub use ring::{Consumer, CycleRing, Producer, RecvOutcome};
pub use writer::{DrainSummary, NdjsonWriter, spawn};
