//! Bounded ring buffer used while the daemon socket is unavailable.
//!
//! REQ_0814: buffer records while daemon is down.
//! REQ_0815: drop-oldest with a count surfaced on next drain.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// A bounded FIFO queue of already-encoded DLT records.
///
/// When the queue is full, [`push`](OfflineRing::push) evicts the
/// oldest entry before inserting the new one, and increments an
/// internal drop counter. The counter is reset to zero by
/// [`drain_all`](OfflineRing::drain_all).
///
/// All operations are safe to call from multiple threads.
#[derive(Debug)]
pub struct OfflineRing {
    inner: Mutex<VecDeque<Vec<u8>>>,
    capacity: usize,
    drops: AtomicU64,
}

impl OfflineRing {
    /// Allocate a ring holding up to `capacity` already-encoded
    /// records. The backing `VecDeque` is sized once at construction
    /// and reused — `push` does not grow it.
    ///
    /// # Panics
    ///
    /// Panics if `capacity` is zero.
    pub fn with_capacity(capacity: usize) -> Self {
        assert!(capacity > 0, "ring capacity must be > 0");
        Self {
            inner: Mutex::new(VecDeque::with_capacity(capacity)),
            capacity,
            drops: AtomicU64::new(0),
        }
    }

    /// Push one record. If the ring is full, evict the oldest entry
    /// and increment the drop counter.
    pub fn push(&self, bytes: Vec<u8>) {
        let mut q = self.inner.lock().unwrap();
        if q.len() == self.capacity {
            q.pop_front();
            self.drops.fetch_add(1, Ordering::Relaxed);
        }
        q.push_back(bytes);
    }

    /// Drain every queued record in FIFO order and reset the drop
    /// counter to zero.
    pub fn drain_all(&self) -> Vec<Vec<u8>> {
        let mut q = self.inner.lock().unwrap();
        let drained: Vec<Vec<u8>> = q.drain(..).collect();
        self.drops.store(0, Ordering::Release);
        drained
    }

    /// Drops accumulated since the last [`drain_all`](OfflineRing::drain_all).
    pub fn drops_since_last_drain(&self) -> u64 {
        self.drops.load(Ordering::Acquire)
    }
}
