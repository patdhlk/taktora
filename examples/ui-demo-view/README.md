# ui-demo-view — egui reference View

A minimal [egui](https://github.com/emilk/egui)/eframe operator panel for the
MVVM UI connector (`FEAT_0092`, Task 6.2). It binds the [`ui-demo`](../ui-demo)
producer **purely over the published JSON contract** via
`taktora-connector-ui-client::Client` — it depends on neither the executor nor
the server crate.

## What it shows

- live `state`, `position`, and the `System` **heartbeat** counter
- a **can_jog** indicator
- an **Enable** button → invokes the idempotent `enable` command
- a **Jog +5** button → invokes the non-idempotent `jog_relative` command; it is
  **grayed out** whenever `can_execute("jog_relative")` is `false`, demonstrating
  `CanExecute` gating end to end
- the outcome of the last command (accepted / rejected+code / outcome-unknown)

## Run it

Start the producer first, then the View:

```sh
cd ../ui-demo && cargo run        # terminal 1: the producer
cd ../ui-demo-view && cargo run   # terminal 2: this View
```

The View probes for the live contract hash, then rebinds read-write. (A real
generated client embeds the hash it was built against; this demo learns it at
runtime for convenience.)

## Dependency scoping

The heavy GUI dependencies (`eframe`, `egui`) live **only in this example
crate** — never in any published library crate, and never in the root workspace
deps (examples are `exclude`d from the workspace). The `glow` (OpenGL) backend is
selected to keep the build lighter than the default wgpu backend.

## Headless note

This is UI glue: it **builds** (`cargo build`) but is not unit tested and cannot
run in a headless/display-less environment. The equivalent client path —
discover → hash-validate → subscribe → poll → invoke, over the demo's exact
model — is verified headlessly by `../ui-demo/tests/e2e.rs`.
