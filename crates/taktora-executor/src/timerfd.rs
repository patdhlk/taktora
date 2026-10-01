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

// Re-export the TimerFd implementation from the sys crate, which contains
// all unsafe FFI code. This module remains a safe interface.
#[allow(clippy::redundant_pub_crate)]
// Inside private module, pub(crate) documents visibility intent
pub(crate) use taktora_executor_sys::os::TimerFd;
