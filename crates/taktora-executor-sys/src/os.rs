//! Linux OS FFI used by the executor (timerfd, timer slack, `SCHED_FIFO`).
//!
//! The module doc and item docs carry many bare technical identifiers
//! (`timerfd`, `epoll`, `hrtimer`, `div_ceil`, syscall/flag names); backticking
//! every one fights readability, so the markdown lint is relaxed module-wide.
#![allow(clippy::doc_markdown)]

#[cfg(target_os = "linux")]
use std::io;
#[cfg(target_os = "linux")]
use std::mem::MaybeUninit;
#[cfg(target_os = "linux")]
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
#[cfg(target_os = "linux")]
use std::time::Duration;

#[cfg(target_os = "linux")]
use iceoryx2_bb_posix::file_descriptor::{FileDescriptor, FileDescriptorBased};
#[cfg(target_os = "linux")]
use iceoryx2_bb_posix::file_descriptor_set::SynchronousMultiplexing;

/// Set the calling thread's timer-slack value to the specified nanoseconds.
///
/// The timer slack controls rounding/deferral of low-resolution timers. Real-time
/// scheduling classes force slack to 0, so this is primarily useful for `SCHED_OTHER`
/// threads to reduce cyclic drift from rounded timeouts.
///
/// # Soundness contract
/// Thread-local operation; never fails for valid arguments. Exotic kernel errors
/// would only restore default behavior, so the return value is deliberately ignored.
#[cfg(target_os = "linux")]
pub fn set_current_thread_timer_slack_ns(ns: u64) {
    // SAFETY: prctl(PR_SET_TIMERSLACK) only adjusts the calling
    // thread's timer slack; no memory is involved.
    unsafe {
        libc::prctl(libc::PR_SET_TIMERSLACK, ns, 0, 0, 0);
    }
}

/// Set the calling thread's scheduling policy to `SCHED_FIFO` with the given priority.
///
/// Requires `CAP_SYS_NICE` or equivalent. Failure (e.g., missing capability) is
/// silently ignored, leaving the thread on its current scheduler.
///
/// # Soundness contract
/// Modifies only the calling thread's scheduler; no memory unsafety.
#[cfg(target_os = "linux")]
pub fn set_current_thread_sched_fifo(prio: i32) {
    let mut param: MaybeUninit<libc::sched_param> = MaybeUninit::zeroed();
    // SAFETY: pthread_setschedparam takes a pointer to sched_param;
    // the param is zero-initialised then we set sched_priority before
    // passing it. Failure (e.g. no CAP_SYS_NICE) is silently ignored.
    unsafe {
        (*param.as_mut_ptr()).sched_priority = prio;
        let _ = libc::pthread_setschedparam(libc::pthread_self(), libc::SCHED_FIFO, param.as_ptr());
    }
}

/// A `CLOCK_MONOTONIC` `timerfd` armed on an absolute periodic grid.
///
/// Owns the underlying fd (closed on drop) and carries a *non-owning* iceoryx2
/// [`FileDescriptor`] view so it can be attached to a `WaitSet` via
/// `attach_notification`. The owning [`OwnedFd`] must outlive any `WaitSet`
/// guard borrowing this `TimerFd` — the dispatch loop keeps it in storage for
/// exactly that lifetime, mirroring the listener-storage discipline.
///
/// Linux `timerfd`-backed absolute-grid cyclic wake source (`REQ_0268`,
/// `ADR_0100`, "Option 2"). A `timerfd` armed with `TFD_TIMER_ABSTIME` sidesteps
/// millisecond rounding: the kernel arms an `hrtimer` that makes the fd readable at
/// a **nanosecond-precise absolute** grid point, and `epoll` wakes on fd-*readiness*
/// (interrupt-driven) rather than a rounded timeout.
#[cfg(target_os = "linux")]
pub struct TimerFd {
    /// Owns the fd; closes it on drop. Used for the overrun `read`.
    owned: OwnedFd,
    /// Non-owning iceoryx2 view over the same fd, for `WaitSet` attachment.
    descriptor: FileDescriptor,
}

#[cfg(target_os = "linux")]
impl TimerFd {
    /// Create a `CLOCK_MONOTONIC` `timerfd` and arm it on the absolute grid:
    /// first expiry at `now + period`, auto-rearming every `period`
    /// (`TFD_TIMER_ABSTIME`, so the kernel keeps the grid phase-locked and
    /// counts overruns). `TFD_NONBLOCK` lets [`Self::drain`] poll without
    /// blocking; `TFD_CLOEXEC` keeps the fd from leaking across `exec`.
    ///
    /// # Soundness contract
    /// Safe for all callers; internally uses FFI to create and configure a timerfd.
    pub fn new(period: Duration) -> io::Result<Self> {
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
        let rc =
            unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, std::ptr::from_mut(&mut now)) };
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
            libc::timerfd_settime(
                raw,
                libc::TFD_TIMER_ABSTIME,
                std::ptr::from_ref(&spec),
                std::ptr::null_mut(),
            )
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
    ///
    /// # Soundness contract
    /// Safe for all callers; internally reads from the owned timerfd.
    pub fn drain(&self) -> u64 {
        let mut buf = [0u8; 8];
        // SAFETY: the fd is open for `self`'s lifetime; `buf` is exactly 8 bytes,
        // the timerfd read size. A short/error read (EAGAIN) yields n != 8.
        let n = unsafe { libc::read(self.owned.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
        if n == 8 { u64::from_ne_bytes(buf) } else { 0 }
    }
}

#[cfg(target_os = "linux")]
impl FileDescriptorBased for TimerFd {
    fn file_descriptor(&self) -> &FileDescriptor {
        &self.descriptor
    }
}

#[cfg(target_os = "linux")]
// SAFETY: a timerfd is a readable fd that becomes ready on expiry,
// exactly the synchronous-multiplexing contract the WaitSet relies on.
impl SynchronousMultiplexing for TimerFd {}

#[cfg(target_os = "linux")]
impl std::fmt::Debug for TimerFd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TimerFd")
            .field("fd", &self.owned.as_raw_fd())
            .finish_non_exhaustive()
    }
}

/// Split a (non-negative) nanosecond count into a `timespec`. Saturates the
/// seconds field at `time_t::MAX` rather than wrapping.
#[cfg(target_os = "linux")]
fn ns_to_timespec(ns: i128) -> libc::timespec {
    let secs = ns / 1_000_000_000;
    let nsec = ns % 1_000_000_000;
    libc::timespec {
        tv_sec: libc::time_t::try_from(secs).unwrap_or(libc::time_t::MAX),
        tv_nsec: nsec as libc::c_long,
    }
}
