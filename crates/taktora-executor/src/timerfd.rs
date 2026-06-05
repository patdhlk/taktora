//! Linux `timerfd`-backed absolute-grid cyclic wake source (`REQ_0268`,
//! `ADR_0100`, "Option 2").
//!
//! The self-computed-timeout approach (`GridTimer` driving
//! `wait_and_process_once_with_timeout`) cannot bound sub-millisecond drift on
//! Linux: iceoryx2's `epoll` `timed_wait` rounds the timeout **up to whole
//! milliseconds** (`iceoryx2-bb-linux` `epoll.rs`: `as_nanos().div_ceil(1e6)`),
//! so the sub-ms correction is quantized away every cycle. Hardware A/B on a
//! Pi5 confirmed grid drifted ~3.3 µs/cycle, identical to the relative timer.
//!
//! A `timerfd` armed with `TFD_TIMER_ABSTIME` sidesteps this entirely: the
//! kernel arms an `hrtimer` that makes the fd readable at a **nanosecond-precise
//! absolute** grid point, and `epoll` wakes on fd-*readiness* (interrupt-driven)
//! rather than the rounded timeout. A Pi5 probe measured slope ≈ 0 ns/cycle
//! (bounded) even under `SCHED_OTHER`. The fd is attached to the executor's
//! `WaitSet` as a notification, so cyclic tasks dispatch through the normal
//! callback path; the loop drains the fd each wake to clear `epoll` readiness.
//!
//! Linux-only: `timerfd` is a Linux facility. Non-Linux targets keep the
//! self-computed-timeout path (development hosts are not the real-time target).

// The module doc and item docs carry many bare technical identifiers
// (`timerfd`, `epoll`, `hrtimer`, `div_ceil`, syscall/flag names); backticking
// every one fights readability, so the markdown lint is relaxed module-wide.
#![allow(clippy::doc_markdown)]

use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::time::Duration;

use iceoryx2_bb_posix::file_descriptor::{FileDescriptor, FileDescriptorBased};
use iceoryx2_bb_posix::file_descriptor_set::SynchronousMultiplexing;

/// A `CLOCK_MONOTONIC` `timerfd` armed on an absolute periodic grid.
///
/// Owns the underlying fd (closed on drop) and carries a *non-owning* iceoryx2
/// [`FileDescriptor`] view so it can be attached to a `WaitSet` via
/// `attach_notification`. The owning [`OwnedFd`] must outlive any `WaitSet`
/// guard borrowing this `TimerFd` — the dispatch loop keeps it in storage for
/// exactly that lifetime, mirroring the listener-storage discipline.
// `pub(crate)` in a private module is intentional: `dispatch_loop` (a sibling
// module) constructs and attaches these.
#[allow(clippy::redundant_pub_crate)]
pub(crate) struct TimerFd {
    /// Owns the fd; closes it on drop. Used for the overrun `read`.
    owned: OwnedFd,
    /// Non-owning iceoryx2 view over the same fd, for `WaitSet` attachment.
    descriptor: FileDescriptor,
}

impl TimerFd {
    /// Create a `CLOCK_MONOTONIC` `timerfd` and arm it on the absolute grid:
    /// first expiry at `now + period`, auto-rearming every `period`
    /// (`TFD_TIMER_ABSTIME`, so the kernel keeps the grid phase-locked and
    /// counts overruns). `TFD_NONBLOCK` lets [`Self::drain`] poll without
    /// blocking; `TFD_CLOEXEC` keeps the fd from leaking across `exec`.
    #[allow(unsafe_code, clippy::borrow_as_ptr)]
    pub(crate) fn new(period: Duration) -> io::Result<Self> {
        // SAFETY: timerfd_create has no preconditions; the flags are valid
        // constants. Returns a fresh owned fd or -1 with errno set.
        let raw = unsafe {
            libc::timerfd_create(
                libc::CLOCK_MONOTONIC,
                libc::TFD_CLOEXEC | libc::TFD_NONBLOCK,
            )
        };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `raw` is a fresh, exclusively-owned fd from timerfd_create.
        let owned = unsafe { OwnedFd::from_raw_fd(raw) };

        let period_ns = i128::try_from(period.as_nanos()).unwrap_or(i128::MAX);
        let mut now = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: clock_gettime writes through the provided pointer; `now` is a
        // valid, properly-aligned timespec.
        let rc = unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut now) };
        if rc < 0 {
            return Err(io::Error::last_os_error());
        }
        let first_ns = i128::from(now.tv_sec) * 1_000_000_000 + i128::from(now.tv_nsec) + period_ns;
        let spec = libc::itimerspec {
            it_value: ns_to_timespec(first_ns),
            it_interval: ns_to_timespec(period_ns),
        };
        // SAFETY: `raw` is open; `spec` is a valid itimerspec; a null old_value
        // is permitted by timerfd_settime.
        let rc = unsafe {
            libc::timerfd_settime(raw, libc::TFD_TIMER_ABSTIME, &spec, std::ptr::null_mut())
        };
        if rc < 0 {
            return Err(io::Error::last_os_error());
        }

        let descriptor = FileDescriptor::non_owning_new(raw)
            .ok_or_else(|| io::Error::other("timerfd: kernel returned an invalid fd"))?;
        Ok(Self { owned, descriptor })
    }

    /// Non-blocking read of the 8-byte expiration counter, clearing the fd's
    /// `epoll` readiness so the next `WaitSet` wait does not spin. Returns the
    /// overrun count since the last drain (0 on `EAGAIN`, i.e. not yet expired).
    #[allow(unsafe_code)]
    pub(crate) fn drain(&self) -> u64 {
        let mut buf = [0u8; 8];
        // SAFETY: the fd is open for `self`'s lifetime; `buf` is exactly 8 bytes,
        // the timerfd read size. A short/error read (EAGAIN) yields n != 8.
        let n = unsafe { libc::read(self.owned.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
        if n == 8 { u64::from_ne_bytes(buf) } else { 0 }
    }
}

impl FileDescriptorBased for TimerFd {
    fn file_descriptor(&self) -> &FileDescriptor {
        &self.descriptor
    }
}

// SAFETY-of-contract: a timerfd is a readable fd that becomes ready on expiry,
// exactly the synchronous-multiplexing contract the WaitSet relies on.
impl SynchronousMultiplexing for TimerFd {}

impl std::fmt::Debug for TimerFd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TimerFd")
            .field("fd", &self.owned.as_raw_fd())
            .finish_non_exhaustive()
    }
}

/// Split a (non-negative) nanosecond count into a `timespec`. Saturates the
/// seconds field at `time_t::MAX` rather than wrapping.
fn ns_to_timespec(ns: i128) -> libc::timespec {
    let secs = ns / 1_000_000_000;
    let nsec = ns % 1_000_000_000;
    libc::timespec {
        tv_sec: libc::time_t::try_from(secs).unwrap_or(libc::time_t::MAX),
        tv_nsec: nsec as libc::c_long,
    }
}

/// Suppress an "unused" warning for `raw_fd` accessor on builds that do not
/// reference it directly (kept for diagnostics/tests).
#[allow(dead_code)]
impl TimerFd {
    pub(crate) fn raw_fd(&self) -> RawFd {
        self.owned.as_raw_fd()
    }
}
