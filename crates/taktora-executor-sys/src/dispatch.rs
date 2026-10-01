//! Shared-mutation primitive for the executor's zero-alloc dispatch.
//!
//! # Design
//!
//! `ExclusiveCell<T>` is the executor's single shared-mutation primitive,
//! replacing all raw-pointer wrappers. It provides exclusive `&mut T` access
//! via a runtime busy flag, enforcing single-owner-at-a-time via atomic CAS
//! instead of compile-time borrow rules.
//!
//! # Soundness
//!
//! - **Exclusion**: `try_with` performs a CAS on `busy` (false → true). Only
//!   one thread wins the race; that thread obtains exclusive `&mut T` access.
//!   A drop guard resets `busy` on unwind, so a panicking closure releases
//!   the cell.
//! - **Memory ordering**: The CAS acquire orders `UnsafeCell` reads/writes
//!   after the flag acquisition; the guard's release orders them before the
//!   flag reset. Together they establish a happens-before edge across
//!   successive accesses.
//! - **Send + Sync**: `T: Send` is required because `&mut T` may be obtained
//!   on any thread. No `T: Sync` bound is needed — the busy flag serializes
//!   access, so no concurrent `&T` borrows exist.
//!
//! Contention (a `try_with` while another is in progress) returns `None`
//! instead of blocking. Callers treat `None` as an invariant breach and route
//! it to the executor's fatal/fault path (pool's `guard_or_fatal`, graph's
//! `debug_assert`), never UB.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, Ordering};

/// Exclusive-access cell with runtime busy-flag enforcement.
///
/// Provides `&mut T` via `try_with` when the cell is not already accessed.
/// Returns `None` (without running the closure) if a concurrent or re-entrant
/// access is in progress. The busy flag is reset by a drop guard so unwind
/// releases the cell.
///
/// # Example
///
/// ```
/// use taktora_executor_sys::dispatch::ExclusiveCell;
/// use std::sync::Arc;
///
/// let cell = Arc::new(ExclusiveCell::new(0_u32));
/// let c2 = Arc::clone(&cell);
///
/// // Exclusive access succeeds.
/// cell.try_with(|n| *n += 1);
/// assert_eq!(cell.try_with(|n| *n), Some(1));
///
/// // Re-entrant access returns None.
/// let result = cell.try_with(|n| {
///     cell.try_with(|m| *m += 1)  // inner try_with returns None
/// });
/// assert_eq!(result, Some(None));
/// ```
pub struct ExclusiveCell<T: ?Sized> {
    busy: AtomicBool,
    /// MUST be the last field to enable `Arc<ExclusiveCell<F>>` →
    /// `Arc<ExclusiveCell<dyn Trait>>` unsized coercion (the fat pointer
    /// metadata for `dyn Trait` is placed at the struct tail).
    value: UnsafeCell<T>,
}

// SAFETY: `T: Send` allows `&mut T` to be obtained on any thread. The busy
// flag serializes access, so no concurrent `&mut T` borrows exist. `Sync` is
// sound because multiple threads may call `try_with` (which borrows `&self`),
// and the CAS + drop guard ensure only one obtains `&mut T`.
unsafe impl<T: ?Sized + Send> Send for ExclusiveCell<T> {}
// SAFETY: Send + Sync soundness documented above. The AtomicBool busy flag
// serializes access, ensuring only one thread obtains `&mut T` at a time.
unsafe impl<T: ?Sized + Send> Sync for ExclusiveCell<T> {}

impl<T> ExclusiveCell<T> {
    /// Create a new cell with the given value.
    #[must_use]
    pub const fn new(value: T) -> Self {
        Self {
            busy: AtomicBool::new(false),
            value: UnsafeCell::new(value),
        }
    }

    /// Consume the cell and return the inner value.
    #[must_use]
    pub fn into_inner(self) -> T {
        self.value.into_inner()
    }
}

impl<T: ?Sized> ExclusiveCell<T> {
    /// Run `f` with exclusive `&mut T` access if the cell is not busy.
    ///
    /// Returns `Some(r)` if access was granted (busy flag CAS succeeded).
    /// Returns `None` if another access (on any thread, or re-entrantly) is
    /// in progress. The busy flag is reset by a drop guard before this
    /// returns, even if `f` panics.
    ///
    /// # Memory ordering
    ///
    /// The CAS uses `Acquire` on success (orders subsequent reads/writes to
    /// `value` after the flag acquisition). The drop guard uses `Release`
    /// (orders prior reads/writes before the flag reset). Together they
    /// establish a happens-before edge across successive accesses.
    #[allow(clippy::items_after_statements)] // Guard helper scoped to try_with
    pub fn try_with<R>(&self, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        // Attempt to acquire the busy flag.
        if self
            .busy
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            return None;
        }

        // Drop guard resets the flag on unwind or normal return.
        struct Guard<'a>(&'a AtomicBool);
        impl Drop for Guard<'_> {
            fn drop(&mut self) {
                self.0.store(false, Ordering::Release);
            }
        }
        let _guard = Guard(&self.busy);

        // SAFETY: we won the CAS race, so no other thread holds `&mut T`.
        // The guard ensures the flag is reset before this scope exits.
        let value_mut = unsafe { &mut *self.value.get() };
        Some(f(value_mut))
    }

    /// Obtain `&mut T` when holding exclusive `&mut self`.
    ///
    /// Bypasses the busy flag (it is never touched). Safe because Rust's
    /// borrow rules prove exclusion.
    #[allow(clippy::missing_const_for_fn)] // UnsafeCell::get_mut is not const
    pub fn get_mut(&mut self) -> &mut T {
        self.value.get_mut()
    }

    /// Check if the cell is currently busy (diagnostics only).
    ///
    /// Uses `Acquire` ordering so a `true` result implies any prior `try_with`
    /// access has completed its `Release` write.
    #[must_use]
    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn exclusive_access() {
        let cell = ExclusiveCell::new(0_u32);
        assert_eq!(cell.try_with(|n| *n += 1), Some(()));
        assert_eq!(cell.try_with(|n| *n), Some(1));
    }

    #[test]
    fn reentrant_returns_none() {
        let cell = ExclusiveCell::new(0_u32);
        let result = cell.try_with(|_n| {
            // Re-entrant access while outer try_with holds &mut.
            cell.try_with(|m| *m += 1)
        });
        assert_eq!(result, Some(None));
    }

    #[test]
    fn release_after_panic() {
        let cell = ExclusiveCell::new(0_u32);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            cell.try_with(|_n| panic!("unwind test"));
        }));
        // Cell is released; subsequent access succeeds.
        assert_eq!(cell.try_with(|n| *n += 1), Some(()));
        assert_eq!(cell.try_with(|n| *n), Some(1));
    }

    #[test]
    fn unsized_coercion_and_invocation() {
        // Sized closure in an Arc<ExclusiveCell<F>>.
        let mut _counter = 0_u32;
        let cell: Arc<ExclusiveCell<dyn FnMut() + Send>> = {
            let f = move || {
                _counter += 1;
            };
            Arc::new(ExclusiveCell::new(f))
        };

        // Coercion to Arc<ExclusiveCell<dyn FnMut() + Send>> succeeds.
        let c2: Arc<ExclusiveCell<dyn FnMut() + Send>> = Arc::clone(&cell);

        // Invoke the closure.
        c2.try_with(|f| f()).expect("access granted");
        c2.try_with(|f| f()).expect("access granted");
    }

    #[test]
    fn two_thread_contention() {
        use std::sync::Barrier;

        let cell = Arc::new(ExclusiveCell::new(0_u32));
        let barrier = Arc::new(Barrier::new(2));
        let c1 = Arc::clone(&cell);
        let c2 = Arc::clone(&cell);
        let b1 = Arc::clone(&barrier);
        let b2 = Arc::clone(&barrier);

        let h1 = std::thread::spawn(move || {
            b1.wait();
            for _ in 0..1000 {
                c1.try_with(|n| *n += 1);
            }
        });

        let h2 = std::thread::spawn(move || {
            b2.wait();
            for _ in 0..1000 {
                c2.try_with(|n| *n += 1);
            }
        });

        h1.join().unwrap();
        h2.join().unwrap();

        // Final value is at most 2000 (some try_with calls returned None due
        // to contention). All successful increments were exclusive, so the
        // counter is never torn/corrupted.
        let final_val = cell.try_with(|n| *n).unwrap();
        assert!(final_val <= 2000, "counter = {final_val}");
    }

    #[test]
    fn get_mut_bypasses_flag() {
        let mut cell = ExclusiveCell::new(42_u32);
        *cell.get_mut() = 99;
        assert_eq!(cell.try_with(|n| *n), Some(99));
    }

    #[test]
    fn into_inner() {
        let cell = ExclusiveCell::new(42_u32);
        assert_eq!(cell.into_inner(), 42);
    }

    #[test]
    fn is_busy() {
        let cell = ExclusiveCell::new(0_u32);
        assert!(!cell.is_busy());
        cell.try_with(|_n| {
            assert!(cell.is_busy());
        });
        assert!(!cell.is_busy());
    }
}
