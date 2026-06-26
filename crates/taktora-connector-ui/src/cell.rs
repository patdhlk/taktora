//! [`SeqlockBytes`]: a single-producer / single-consumer **latest-value** cell
//! holding the integer-lowered image bytes of one ViewModel.
//!
//! The RT producer (the control task) calls [`SeqlockBytes::write`] once per
//! cycle; the non-RT pump calls [`SeqlockBytes::read_into`]. Neither path
//! allocates: the backing `Box<[u8]>` is sized once in
//! [`SeqlockBytes::with_len`], and the reader copies into a caller-owned buffer.
//!
//! This is the latest-value (history-depth-1) analogue of the overwrite-oldest
//! ring in `taktora-telemetry-export`; it follows the same seqlock discipline.
//!
//! # Seqlock soundness
//!
//! The consumer may read the cell's bytes while the producer overwrites them;
//! the read is validated against the cell's sequence number afterward and
//! discarded if torn. The cell stores **raw bytes** — the integer-lowered image
//! produced by [`ViewModel::to_image`](crate::ViewModel::to_image), never a
//! typed value with enum fields — so a torn read is always a valid bit pattern
//! (no invalid enum discriminant) and, because the payload is plain `u8`, never
//! triggers `Drop`. A caller reconstructs a typed image from these bytes only
//! *after* [`read_into`](SeqlockBytes::read_into) reports a tear-free read, so
//! the reconstructed value is always one the producer actually wrote. The
//! residual strict-aliasing caveat of the racy read is inherent to the seqlock
//! pattern; a fully MIRI-clean per-word-atomic variant is deferred.
//!
//! The single-producer invariant is **type-enforced** one level up:
//! [`SeqlockBytes::write`] is `pub(crate)`, reachable only through the move-only
//! [`Property`](crate::Property) handle, of which exactly one can exist per cell
//! (it is not `Clone`). [`SeqlockBytes`] is `Sync` so that `Property` and the
//! clone-able [`PropertyReader`](crate::property::PropertyReader)s can hold it
//! across threads; concurrent *readers* are sound for a seqlock, and the unique
//! writer rules out the two-writer data race at the type level.

// The seqlock requires `unsafe` to mediate the racy read of `UnsafeCell` bytes
// across threads; the crate otherwise forbids it via `#![deny(unsafe_code)]`.
// Every `unsafe` block below is justified by the per-cell sequence number that
// brackets the producer's non-atomic write and lets the consumer validate and
// discard a torn read (see the module-level "Seqlock soundness" note).
#![allow(unsafe_code)]

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicU64, Ordering, fence};

/// High bit of the sequence word, set while the producer is writing the
/// payload. Real sequence numbers never reach `1 << 63`.
const WRITING: u64 = 1 << 63;

/// Bound on reader retries before [`SeqlockBytes::read_into`] gives up and
/// reports the cell as torn.
///
/// Under the single-producer / single-consumer contract a reader can be lapped
/// by at most one in-flight write per attempt, so a tear-free snapshot is
/// reached in a couple of iterations in the common case. The bound exists only
/// to keep the non-RT reader from spinning forever if the producer is parked
/// mid-write (e.g. a crashed or descheduled RT task); the reader returns
/// `false` rather than block.
const READ_RETRY_LIMIT: u32 = 64;

/// A latest-value seqlock cell holding `len` bytes (the integer-lowered image
/// of one ViewModel). Single producer, single consumer; never blocks.
pub(crate) struct SeqlockBytes {
    /// Sequence of the bytes currently in `buf`, with [`WRITING`] set
    /// transiently while they are being overwritten. `0` means "never written".
    seq: AtomicU64,
    /// Producer-owned shadow of `seq` (without [`WRITING`]). Avoids re-deriving
    /// the next sequence from the published atomic, keeping `write` a single
    /// reader of its own counter.
    write_seq: UnsafeCell<u64>,
    /// The image bytes, guarded by the seqlock on `seq`.
    buf: UnsafeCell<Box<[u8]>>,
    /// The fixed image length (`buf.len()`), cached for the reader.
    len: usize,
}

// SAFETY: `SeqlockBytes` is shared between exactly one producer and one
// consumer. The `UnsafeCell` payload is guarded by the per-cell seqlock
// (`seq`): the producer brackets its non-atomic write with a `WRITING` marker
// and a publishing store; the consumer validates `seq` before and after its
// read and discards torn reads. The payload is plain bytes (`Copy`, no `Drop`),
// so a torn read is harmless. `write_seq` is touched only by the single
// producer. See the module-level soundness note.
unsafe impl Sync for SeqlockBytes {}

impl SeqlockBytes {
    /// Allocate a cell holding `n` image bytes. This is the **only** allocation;
    /// neither [`write`](Self::write) nor [`read_into`](Self::read_into)
    /// allocates afterward.
    #[must_use]
    pub fn with_len(n: usize) -> Self {
        Self {
            seq: AtomicU64::new(0),
            write_seq: UnsafeCell::new(0),
            buf: UnsafeCell::new(vec![0u8; n].into_boxed_slice()),
            len: n,
        }
    }

    /// Publish `src` as the latest value. Wait-free, allocation-free. Must be
    /// called from a single thread (the RT producer); `pub(crate)` so only the
    /// move-only [`Property`](crate::Property) can drive the producer side.
    ///
    /// # Panics
    ///
    /// Panics if `src.len()` differs from the cell length. This is a real
    /// `assert_eq!` (negligible next to the byte copy below): it is
    /// defense-in-depth against an out-of-bounds `copy_nonoverlapping`, since
    /// the copy length is `self.len` and a shorter `src` would read OOB.
    pub(crate) fn write(&self, src: &[u8]) {
        assert_eq!(src.len(), self.len, "image length mismatch");

        // SAFETY: `write_seq` is producer-owned (single-producer contract), so
        // this is the only thread reading or writing it; no aliasing.
        let next = unsafe {
            let w = self.write_seq.get();
            let n = (*w) + 1;
            *w = n;
            n
        };

        // Mark "writing", then perform the non-atomic payload write, then
        // publish the new sequence.
        self.seq.store(next | WRITING, Ordering::Release);
        // This release fence orders the `WRITING` marker store *before* the
        // non-atomic payload write below: a `Release` store only bars earlier
        // ops from sinking past it, not later ops from being hoisted above it.
        // Without the fence, a weakly-ordered target (e.g. AArch64) could make
        // the payload mutation visible while `seq` still shows the previous
        // value, letting a consumer pass its `s1 == s2` tear check on spliced
        // bytes. Not redundant.
        fence(Ordering::Release);
        // SAFETY: single producer; the `WRITING` marker is visible to the
        // consumer (Release/Acquire on `seq`), which discards any read racing
        // this store. `src.len() == self.len == buf.len()`.
        unsafe {
            let dst = (*self.buf.get()).as_mut_ptr();
            std::ptr::copy_nonoverlapping(src.as_ptr(), dst, self.len);
        }
        self.seq.store(next, Ordering::Release);
    }

    /// Copy the latest tear-free value into `dst`, reusing its allocation.
    ///
    /// `dst` is resized to the cell length on first use and reused thereafter
    /// (alloc-free once warm). Returns `true` if a tear-free value was copied;
    /// returns `false` if the cell was never written or stayed torn for
    /// [`READ_RETRY_LIMIT`] attempts (e.g. the producer is parked mid-write).
    /// When `false` is returned the contents of `dst` are unspecified.
    ///
    /// Must be called from a single thread (the non-RT pump).
    pub fn read_into(&self, dst: &mut Vec<u8>) -> bool {
        if dst.len() != self.len {
            dst.resize(self.len, 0);
        }

        for _ in 0..READ_RETRY_LIMIT {
            let s1 = self.seq.load(Ordering::Acquire);
            if s1 & WRITING != 0 {
                // Producer is mid-write; re-evaluate.
                continue;
            }
            if s1 == 0 {
                // Never written.
                return false;
            }
            // SAFETY: guarded by the seqlock — we re-check `seq` after the copy
            // and discard a torn read below. The payload is plain bytes (no
            // `Drop`); `dst.len() == self.len == buf.len()`.
            unsafe {
                let s = (*self.buf.get()).as_ptr();
                std::ptr::copy_nonoverlapping(s, dst.as_mut_ptr(), self.len);
            }
            fence(Ordering::Acquire);
            let s2 = self.seq.load(Ordering::Acquire);
            if s1 == s2 {
                return true;
            }
            // Torn (or `WRITING` set in s2): the value changed during the copy;
            // retry.
        }
        false
    }

    /// Convenience reader that allocates a fresh `Vec`. Test-only; the RT/pump
    /// paths use [`read_into`](Self::read_into) to stay alloc-free.
    #[cfg(test)]
    #[must_use]
    pub fn read(&self) -> Option<Vec<u8>> {
        let mut buf = Vec::new();
        if self.read_into(&mut buf) {
            Some(buf)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_returns_none_until_first_write() {
        let cell = SeqlockBytes::with_len(8);
        assert_eq!(cell.read(), None);
    }

    #[test]
    fn read_returns_latest_fully_written_image() {
        let cell = SeqlockBytes::with_len(8);
        cell.write(&[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(cell.read(), Some(vec![1, 2, 3, 4, 5, 6, 7, 8]));
        cell.write(&[9, 9, 9, 9, 9, 9, 9, 9]);
        assert_eq!(cell.read(), Some(vec![9; 8]));
    }

    #[test]
    fn read_into_reuses_caller_buffer() {
        let cell = SeqlockBytes::with_len(4);
        let mut buf = Vec::new();
        assert!(!cell.read_into(&mut buf));
        cell.write(&[10, 20, 30, 40]);
        assert!(cell.read_into(&mut buf));
        assert_eq!(buf, vec![10, 20, 30, 40]);
    }

    #[test]
    fn read_under_concurrent_writes_is_never_torn() {
        use std::sync::Arc;
        use std::sync::atomic::AtomicBool;
        use std::thread;

        const N: usize = 64;
        let cell = Arc::new(SeqlockBytes::with_len(N));
        let stop = Arc::new(AtomicBool::new(false));

        let writer_cell = Arc::clone(&cell);
        let writer_stop = Arc::clone(&stop);
        let writer = thread::spawn(move || {
            let mut k: u8 = 0;
            while !writer_stop.load(Ordering::Relaxed) {
                writer_cell.write(&[k; N]);
                k = k.wrapping_add(1);
            }
        });

        let mut buf = Vec::new();
        let mut successful_reads: u64 = 0;
        for _ in 0..200_000 {
            if cell.read_into(&mut buf) {
                successful_reads += 1;
                let first = buf[0];
                assert!(
                    buf.iter().all(|&b| b == first),
                    "torn read observed: {buf:?}"
                );
            }
        }

        stop.store(true, Ordering::Relaxed);
        writer.join().unwrap();

        // Guard against a vacuous pass: if the loop never observed a tear-free
        // value the "never torn" assertion above would be trivially satisfied.
        assert!(
            successful_reads > 0,
            "stress test made no successful reads — torn-read assertion was vacuous"
        );
    }
}
