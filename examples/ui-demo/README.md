# ui-demo — MVVM UI connector, hardware-free producer

The **v1 validation slice** of the MVVM UI connector (`FEAT_0092`), with **no
fieldbus and no hardware**. A simulated stepper axis stands in for a real control
loop, exposing exactly the MVVM surface a real driver would:

- a `Stepper` **ViewModel** property — `{ position: f64, state: StepperState, can_jog: bool }`
- an **idempotent** `enable` command (`Idle → Running`)
- a **non-idempotent** `jog_relative { delta: f64 }` command, gated by `CanExecute`
- the mandatory `System` **heartbeat** ViewModel (`{ counter, epoch }`), published automatically

This is the producer side. The reference egui **View** lives in
[`../ui-demo-view`](../ui-demo-view). The language-neutral **Python** smoke
consumer and the contract-hash reproducibility proof live with the contract
crate itself (`crates/taktora-connector-ui-contract/py/` +
`tests/golden.rs`), so this example stays focused on the runnable producer.

## Why a standalone demo instead of wiring into `ethercat-stepper`

The plan's Task 6.1 targets `examples/ethercat-stepper`, but that example is
coupled to real EtherCAT hardware: it pulls `taktora-connector-ethercat` with the
`bus-integration` feature (bindgen + clang-sys), and its `build.rs` runs the ESI
device-driver codegen toolchain. It cannot be *run* without a bus, and is heavy
to even build here.

A standalone, hardware-free demo is therefore the more valuable verifiable
artifact: it **builds and runs** anywhere, and it exercises the full connector
path (property publish + coalescing pump, command accept/dedupe, `CanExecute`
gating, the `System` heartbeat, manifest + contract hash) end to end.

## Dependency wiring

This example follows the repo's example convention: every `taktora-*` dependency
pins the **published** major in `[dependencies]`, and a commented
`[patch.crates-io]` block (toggled by `scripts/examples-local.sh on|off`)
redirects them at the in-tree `crates/` during local development. The patch block
lists every taktora crate in the graph — direct **and** transitive — so a single
`taktora-executor` (and a single shared `taktora-connector-core`, etc.) spans the
whole graph; a version split there would fail `register_with` with a type
mismatch. CI requires the committed state to be `off`.

## Run it

```sh
cd examples/ui-demo
cargo run                 # starts the producer on instance "ui-demo" (Ctrl-C to stop)
cargo run -- --ticks 5    # run exactly 5 control ticks then exit 0 (the CI smoke)
```

The axis starts **Idle**. Press **Enable** from the View (or invoke `enable`) to
start the position ramp; `can_jog` toggles on its own every few seconds so you
can watch the View's Jog button gray out and re-enable.

## Tests

```sh
cargo test           # lib unit tests + headless e2e
```

- `src/lib.rs` — the pure `Simulator` + domain model, unit tested (no iceoryx2).
- `tests/e2e.rs` — stands up the producer's connector + executor in-process and
  drives a real `taktora-connector-ui-client::Client` through discover →
  hash-validate → subscribe → invoke, over the demo's exact model. This is the
  headless proxy for the egui View, which cannot run without a display.
