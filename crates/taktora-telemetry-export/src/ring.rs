//! Single-producer / single-consumer **overwrite-oldest** ring with per-slot
//! sequence numbers (a seqlock). The producer never blocks; a lagging consumer
//! is *lapped* and the lost count is reported, never silently dropped.
//!
//! # Invariants
//!
//! * Exactly one thread calls [`Producer::push`] (the executor `WaitSet`
//!   thread).
//! * Exactly one thread calls [`Consumer::try_recv`] (the drain thread).
//! * Capacity is rounded up to a power of two so slot indexing is a mask.
//!
//! See the crate-level docs for the seqlock soundness rationale (POD payload,
//! torn reads validated and discarded).

use std::cell::UnsafeCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering, fence};

use crate::record::PodRecord;

/// High bit of a slot's sequence word, set while the producer is writing the
/// slot's payload. Real sequence numbers never reach `1 << 63`.
const WRITING: u64 = 1 << 63;

struct Slot {
    /// Global sequence of the record currently in `rec`, with [`WRITING`] set
    /// transiently while the payload is being overwritten.
    seq: AtomicU64,
    rec: UnsafeCell<PodRecord>,
}

struct Shared {
    mask: u64,
    /// Total number of records published so far; also the global sequence the
    /// next push will use. Producer-owned (only `push` stores it).
    head: AtomicU64,
    slots: Box<[Slot]>,
}

// SAFETY: `Shared` is shared between exactly one producer and one consumer.
// The `UnsafeCell<PodRecord>` payloads are guarded by the per-slot seqlock
// (`seq`): the producer brackets its non-atomic write with a `WRITING` marker
// and a publishing store; the consumer validates `seq` before and after its
// read and discards torn reads. `PodRecord: Copy` (no `Drop`), so a torn read
// is harmless.
unsafe impl Sync for Shared {}
unsafe impl Send for Shared {}

/// A bounded overwrite-oldest telemetry ring. Build with
/// [`with_capacity`](Self::with_capacity), then [`split`](Self::split) into a
/// [`Producer`] / [`Consumer`] pair.
pub struct CycleRing {
    shared: Arc<Shared>,
}

impl CycleRing {
    /// Create a ring with at least `capacity` slots (rounded up to a power of
    /// two, minimum 2). Slots are preallocated; `push` never allocates.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        let cap = capacity.max(2).next_power_of_two();
        let mut slots = Vec::with_capacity(cap);
        for _ in 0..cap {
            slots.push(Slot {
                // Sentinel `WRITING` with sequence 0 marks "never written":
                // a consumer never expects to read sequence `WRITING`.
                seq: AtomicU64::new(WRITING),
                rec: UnsafeCell::new(PodRecord::new_faulted(0, 0, 0, 0)),
            });
        }
        Self {
            shared: Arc::new(Shared {
                mask: (cap as u64) - 1,
                head: AtomicU64::new(0),
                slots: slots.into_boxed_slice(),
            }),
        }
    }

    /// Split into the producer and consumer halves.
    #[must_use]
    pub fn split(self) -> (Producer, Consumer) {
        let producer = Producer {
            shared: Arc::clone(&self.shared),
        };
        let consumer = Consumer {
            shared: self.shared,
            next: 0,
        };
        (producer, consumer)
    }
}

/// Producer half. `push` is wait-free and must be called from a single thread.
pub struct Producer {
    shared: Arc<Shared>,
}

impl Producer {
    /// Publish one record, overwriting the oldest slot if the ring is full.
    /// Never blocks, never allocates.
    pub fn push(&self, rec: PodRecord) {
        // `head` is producer-owned, so a Relaxed load of our own value is fine.
        let w = self.shared.head.load(Ordering::Relaxed);
        // `w & mask` is `< cap`, and `cap` came from a `usize`, so it round-trips.
        #[allow(clippy::cast_possible_truncation)]
        let idx = (w & self.shared.mask) as usize;
        let slot = &self.shared.slots[idx];

        // Mark the slot "writing" so a concurrent reader bails or detects the
        // change, then perform the non-atomic payload write, then publish.
        slot.seq.store(w | WRITING, Ordering::Release);
        fence(Ordering::Release);
        // SAFETY: single producer; the `WRITING` marker is visible to the
        // consumer (Release/Acquire on `seq`), which discards any read that
        // races this store.
        unsafe {
            *slot.rec.get() = rec;
        }
        slot.seq.store(w, Ordering::Release);

        // Publish the new head last so the consumer never observes a head that
        // outruns a slot's published sequence.
        self.shared.head.store(w + 1, Ordering::Release);
    }
}

/// Consumer half. `try_recv` must be called from a single thread.
pub struct Consumer {
    shared: Arc<Shared>,
    /// Next global sequence we want to deliver.
    next: u64,
}

/// Outcome of one [`Consumer::try_recv`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecvOutcome {
    /// A tear-free record was delivered.
    Record(PodRecord),
    /// The consumer fell behind and `skipped` records were overwritten before
    /// they could be read. `next` has been advanced past them.
    Lapped {
        /// Number of records lost to overwrite.
        skipped: u64,
    },
    /// Nothing new since the last call (caught up to the producer).
    Empty,
}

impl Consumer {
    /// Try to receive the next record. Non-blocking.
    pub fn try_recv(&mut self) -> RecvOutcome {
        let cap = self.shared.mask + 1;
        loop {
            let head = self.shared.head.load(Ordering::Acquire);
            if self.next >= head {
                return RecvOutcome::Empty;
            }

            // Lap check: the oldest still-resident sequence is `head - cap`.
            // If we want something older, it was overwritten.
            if head - self.next > cap {
                let oldest = head - cap;
                let skipped = oldest - self.next;
                self.next = oldest;
                return RecvOutcome::Lapped { skipped };
            }

            // `next & mask` is `< cap`, and `cap` came from a `usize`.
            #[allow(clippy::cast_possible_truncation)]
            let idx = (self.next & self.shared.mask) as usize;
            let slot = &self.shared.slots[idx];
            let s1 = slot.seq.load(Ordering::Acquire);
            if s1 & WRITING != 0 {
                // Producer is mid-write on this slot; re-evaluate.
                continue;
            }
            if s1 != self.next {
                // The slot already moved past `next` (producer lapped us between
                // the head load and here); recompute the lap from a fresh head.
                continue;
            }
            // SAFETY: guarded by the seqlock — we re-check `seq` after the read
            // and discard a torn copy. `PodRecord: Copy`, no `Drop`.
            let rec = unsafe { std::ptr::read(slot.rec.get()) };
            fence(Ordering::Acquire);
            let s2 = slot.seq.load(Ordering::Acquire);
            if s1 != s2 {
                // Slot was overwritten during the read; retry.
                continue;
            }
            self.next += 1;
            return RecvOutcome::Record(rec);
        }
    }
}
