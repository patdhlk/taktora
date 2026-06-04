//! PREEMPT_RT jitter bench harness — `idle` profile (REQ_0111).
//!
//! Runs a single cyclic task under the executor's real-time clock, exports
//! every per-cycle observation off the RT thread through the telemetry ring,
//! and writes NDJSON. `cpu-stress` / `cyclictest-coexist` profiles are a
//! deferred follow-up.
//!
//! Usage:
//! ```text
//! preempt-rt-bench [--cycles N] [--period-us U] [--ring-capacity C] [--out PATH|-]
//! ```
//! Defaults: cycles=5000, period-us=1000, ring-capacity=65536, out=- (stdout).

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use taktora_executor::{ControlFlow, Executor, item_with_triggers};
use taktora_telemetry_export::{CycleRing, NdjsonRingObserver, spawn};

struct Args {
    cycles: usize,
    period_us: u64,
    ring_capacity: usize,
    out: String,
}

impl Args {
    fn parse() -> Result<Self, String> {
        let mut a = Args {
            cycles: 5000,
            period_us: 1000,
            ring_capacity: 65536,
            out: "-".to_string(),
        };
        let mut it = std::env::args().skip(1);
        while let Some(flag) = it.next() {
            let mut next = || it.next().ok_or_else(|| format!("missing value for {flag}"));
            match flag.as_str() {
                "--cycles" => a.cycles = next()?.parse().map_err(|e| format!("--cycles: {e}"))?,
                "--period-us" => {
                    a.period_us = next()?.parse().map_err(|e| format!("--period-us: {e}"))?;
                }
                "--ring-capacity" => {
                    a.ring_capacity = next()?
                        .parse()
                        .map_err(|e| format!("--ring-capacity: {e}"))?;
                }
                "--out" => a.out = next()?,
                "-h" | "--help" => return Err("help".to_string()),
                other => return Err(format!("unknown flag: {other}")),
            }
        }
        Ok(a)
    }
}

fn run() -> Result<(), String> {
    let args = Args::parse()?;

    let sink: Box<dyn Write + Send> = if args.out == "-" {
        Box::new(BufWriter::new(io::stdout()))
    } else {
        Box::new(BufWriter::new(
            File::create(&args.out).map_err(|e| format!("create {}: {e}", args.out))?,
        ))
    };

    let (producer, consumer) = CycleRing::with_capacity(args.ring_capacity).split();
    let observer: Arc<dyn taktora_executor::Observer> = Arc::new(NdjsonRingObserver::new(producer));
    let writer = spawn(consumer, sink);

    // Default builder uses the SystemClock real-time telemetry source.
    let mut exec = Executor::builder()
        .worker_threads(0)
        .observer(observer)
        .stats_window(1024)
        .build()
        .map_err(|e| format!("build executor: {e}"))?;

    let period = Duration::from_micros(args.period_us);
    exec.add(item_with_triggers(
        move |d| {
            d.interval(period);
            Ok(())
        },
        // Idle body: do nothing measurable; we are sampling dispatch timing.
        |_ctx| Ok(ControlFlow::Continue),
    ))
    .map_err(|e| format!("add task: {e}"))?;

    exec.run_n(args.cycles).map_err(|e| format!("run: {e}"))?;

    let summary = writer.finish().map_err(|e| format!("drain: {e}"))?;
    eprintln!(
        "preempt-rt-bench: wrote {} records, {} lapped (ring capacity {})",
        summary.written, summary.lapped, args.ring_capacity
    );
    if summary.lapped > 0 {
        eprintln!(
            "preempt-rt-bench: WARNING {} samples lapped — increase --ring-capacity for a clean envelope",
            summary.lapped
        );
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) if msg == "help" => {
            eprintln!(
                "usage: preempt-rt-bench [--cycles N] [--period-us U] [--ring-capacity C] [--out PATH|-]"
            );
            ExitCode::SUCCESS
        }
        Err(msg) => {
            eprintln!("preempt-rt-bench: {msg}");
            ExitCode::FAILURE
        }
    }
}
