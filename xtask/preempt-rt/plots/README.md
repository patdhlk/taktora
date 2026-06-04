# Offline jitter plots

These gnuplot scripts render the NDJSON produced by `preempt-rt-bench` into
PNGs. They are **offline, dev-time tools** — the bench binary itself never
calls gnuplot, and neither gnuplot nor `jq` is a build/run dependency of the
workspace.

## Prerequisites

- `gnuplot` (with the `pngcairo` terminal)
- `jq`

## Capture a run

```bash
cargo run -p xtask-preempt-rt --release -- \
  --cycles 100000 --period-us 1000 --ring-capacity 131072 --out run.ndjson
```

The harness prints a summary to stderr, e.g.
`preempt-rt-bench: wrote 100000 records, 0 lapped (ring capacity 131072)`.
A non-zero **lapped** count means the drain thread fell behind and the envelope
is incomplete — increase `--ring-capacity` and re-run for a clean measurement.

## Render

```bash
gnuplot -e "data='run.ndjson'" jitter-trace.gp   # -> jitter-trace.png
gnuplot -e "data='run.ndjson'" jitter-cdf.gp     # -> jitter-cdf.png
```

## NDJSON schema

One record per scan cycle (REQ_0111, faithful form):

```json
{"cycle_index":0,"task_id":0,"faulted":false,"ts_ns":12345,"period_ns":1000000,
 "actual_period_ns":1000010,"jitter_ns":10,"lateness_ns":-3,"took_ns":250}
```

Absent measurements (first cycle, faulted scans) render as `null` — the scripts
filter them with `select(.jitter_ns!=null)`.
