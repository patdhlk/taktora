//! `PodRecord` — the plain-old-data ring element and NDJSON source.
//!
//! Integers + a presence bitmask only (no enums), so a torn seqlock read is
//! always a valid bit pattern (see crate-level docs). Built from a
//! [`CycleObservation`] at the producer boundary; serialized to NDJSON on the
//! drain thread.
//!
//! [`CycleObservation`]: taktora_executor::CycleObservation

use std::io::{self, Write};

use taktora_executor::CycleObservation;

// Presence bits in `PodRecord::flags`. Bit 0 is the faulted flag; the rest
// mark which optional measurements were actually taken this cycle.
const F_FAULTED: u32 = 1 << 0;
const F_ACTUAL_PERIOD: u32 = 1 << 1;
const F_JITTER: u32 = 1 << 2;
const F_LATENESS: u32 = 1 << 3;
const F_TOOK: u32 = 1 << 4;

/// A flattened, `Copy` snapshot of one [`CycleObservation`] suitable for a
/// seqlock ring slot. Absent measurements are encoded by a clear presence bit
/// in [`flags`](Self::flags), never by a sentinel value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PodRecord {
    /// Monotonic per-task scan counter (`REQ_0107`).
    pub cycle_index: u64,
    /// Telemetry-clock instant of task-logic start (ns) — the time axis.
    pub ts_ns: u64,
    /// Declared nominal period (ns); always present.
    pub period_ns: u64,
    /// Measured period (ns); valid iff `F_ACTUAL_PERIOD` set.
    pub actual_period_ns: u64,
    /// Absolute jitter (ns); valid iff `F_JITTER` set.
    pub jitter_ns: u64,
    /// Signed deadline lateness (ns); valid iff `F_LATENESS` set.
    pub lateness_ns: i64,
    /// Execute duration (ns); valid iff `F_TOOK` set.
    pub took_ns: u64,
    /// Stable task registration index (`REQ_0111` `task_id` column).
    pub task_index: u32,
    /// Faulted flag + per-field presence bits.
    pub flags: u32,
}

impl PodRecord {
    /// Build from an executor push observation. Reads `pre_ns` for the time
    /// axis and `task_index` for identity (both added in `REQ_0103`).
    #[must_use]
    pub const fn from_observation(obs: &CycleObservation) -> Self {
        let mut flags = 0;
        if obs.faulted {
            flags |= F_FAULTED;
        }
        let mut actual_period_ns = 0;
        if let Some(v) = obs.actual_period_ns {
            actual_period_ns = v;
            flags |= F_ACTUAL_PERIOD;
        }
        let mut jitter_ns = 0;
        if let Some(v) = obs.jitter_ns {
            jitter_ns = v;
            flags |= F_JITTER;
        }
        let mut lateness_ns = 0;
        if let Some(v) = obs.lateness_ns {
            lateness_ns = v;
            flags |= F_LATENESS;
        }
        let mut took_ns = 0;
        if let Some(v) = obs.took_ns {
            took_ns = v;
            flags |= F_TOOK;
        }
        Self {
            cycle_index: obs.cycle_index,
            ts_ns: obs.pre_ns,
            period_ns: obs.period_ns,
            actual_period_ns,
            jitter_ns,
            lateness_ns,
            took_ns,
            task_index: obs.task_index,
            flags,
        }
    }

    /// Test/constructor helper: a fully-measured (healthy) record.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub const fn new_healthy(
        cycle_index: u64,
        task_index: u32,
        ts_ns: u64,
        period_ns: u64,
        actual_period_ns: u64,
        jitter_ns: u64,
        lateness_ns: i64,
        took_ns: u64,
    ) -> Self {
        Self {
            cycle_index,
            ts_ns,
            period_ns,
            actual_period_ns,
            jitter_ns,
            lateness_ns,
            took_ns,
            task_index,
            flags: F_ACTUAL_PERIOD | F_JITTER | F_LATENESS | F_TOOK,
        }
    }

    /// Test/constructor helper: a faulted record (nothing measured).
    #[must_use]
    pub const fn new_faulted(
        cycle_index: u64,
        task_index: u32,
        ts_ns: u64,
        period_ns: u64,
    ) -> Self {
        Self {
            cycle_index,
            ts_ns,
            period_ns,
            actual_period_ns: 0,
            jitter_ns: 0,
            lateness_ns: 0,
            took_ns: 0,
            task_index,
            flags: F_FAULTED,
        }
    }

    /// `true` if this record is a faulted scan.
    #[must_use]
    pub const fn faulted(&self) -> bool {
        self.flags & F_FAULTED != 0
    }

    /// Write one NDJSON line (terminated by `\n`). Absent measurements render
    /// as JSON `null` (`set datafile missing` in gnuplot skips them).
    pub fn write_ndjson<W: Write>(&self, w: &mut W) -> io::Result<()> {
        write!(
            w,
            "{{\"cycle_index\":{},\"task_id\":{},\"faulted\":{},\"ts_ns\":{},\"period_ns\":{},",
            self.cycle_index,
            self.task_index,
            self.faulted(),
            self.ts_ns,
            self.period_ns,
        )?;
        w.write_all(b"\"actual_period_ns\":")?;
        put_u64(w, self.flags & F_ACTUAL_PERIOD != 0, self.actual_period_ns)?;
        w.write_all(b",\"jitter_ns\":")?;
        put_u64(w, self.flags & F_JITTER != 0, self.jitter_ns)?;
        w.write_all(b",\"lateness_ns\":")?;
        put_i64(w, self.flags & F_LATENESS != 0, self.lateness_ns)?;
        w.write_all(b",\"took_ns\":")?;
        put_u64(w, self.flags & F_TOOK != 0, self.took_ns)?;
        w.write_all(b"}\n")
    }
}

fn put_u64<W: Write>(w: &mut W, present: bool, v: u64) -> io::Result<()> {
    if present {
        write!(w, "{v}")
    } else {
        w.write_all(b"null")
    }
}

fn put_i64<W: Write>(w: &mut W, present: bool, v: i64) -> io::Result<()> {
    if present {
        write!(w, "{v}")
    } else {
        w.write_all(b"null")
    }
}
