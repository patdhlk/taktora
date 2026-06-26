# ui-demo — MVVM UI connector, hardware-free producer

The **v1 validation slice** of the MVVM UI connector (`FEAT_0092`), with **no
fieldbus and no hardware**. A simulated stepper axis stands in for a real control
loop, exposing exactly the MVVM surface a real driver would:

- a `Stepper` **ViewModel** property — `{ position: f64, state: StepperState, can_jog: bool }`
- an **idempotent** `enable` command (`Idle → Running`)
- a **non-idempotent** `jog_relative { delta: f64 }` command, gated by `CanExecute`
- the mandatory `System` **heartbeat** ViewModel (`{ counter, epoch }`), published automatically

This is the producer side. The reference egui **View** lives in
[`../ui-demo-view`](../ui-demo-view); a language-neutral **Python** smoke
consumer lives in [`py/`](py).

## Why a standalone demo instead of wiring into `ethercat-stepper`

The plan's Task 6.1 targets `examples/ethercat-stepper`, but that example is
coupled to real EtherCAT hardware: it pulls `taktora-connector-ethercat` with the
`bus-integration` feature (bindgen + clang-sys), and its `build.rs` runs the ESI
device-driver codegen toolchain. It cannot be *run* without a bus, and is heavy
to even build here. On top of that, the UI connector crates are brand-new and
**unpublished** (`0.1.0` on this branch), so they cannot use the published
`version` + toggled `[patch.crates-io]` convention the other examples use — they
must be referenced by `path`.

A standalone, hardware-free demo is therefore the more valuable verifiable
artifact: it **builds and runs** anywhere, and it exercises the full connector
path (property publish + coalescing pump, command accept/dedupe, `CanExecute`
gating, the `System` heartbeat, manifest + contract hash) end to end.

## Dependency wiring

Every taktora crate is a `path` dependency (see `Cargo.toml` for the rationale):
the UI crates are unpublished, and using a single `taktora-executor` across the
graph avoids the version-split that would fail `register_with` with a type
mismatch. There is **no** `examples-local.sh` patch block — this example always
builds against the local crates.

## Run it

```sh
cd examples/ui-demo
cargo run            # starts the producer on instance "ui-demo"
```

The axis starts **Idle**. Press **Enable** from the View (or invoke `enable`) to
start the position ramp; `can_jog` toggles on its own every few seconds so you
can watch the View's Jog button gray out and re-enable.

## Tests

```sh
cargo test           # lib unit tests + contract reproducibility + headless e2e
```

- `src/lib.rs` — the pure `Simulator` + domain model, unit tested (no iceoryx2).
- `tests/contract_reproducible.rs` — recomputes the golden manifest's
  `contract_hash` from the canonical algorithm and asserts it matches (the
  language-neutrality proof; see Task 6.3 / `py/`).
- `tests/e2e.rs` — stands up the producer's connector + executor in-process and
  drives a real `taktora-connector-ui-client::Client` through discover →
  hash-validate → subscribe → invoke, over the demo's exact model. This is the
  headless proxy for the egui View, which cannot run without a display.
