---
name: add-connector
description: >-
  Use when adding a new protocol connector to the taktora-connector framework
  (e.g. "add an MQTT connector", "scaffold a new connector crate", "create a
  taktora-connector-<proto>"). Carries the mechanical steps and the taktora-
  specific gotchas that the prose guide does not spell out as commands.
---

# add-connector

This skill is the **action companion** to the connector guide. The guide is the
source of truth for *why* and *how the framework fits together*; read it first:

> `docs/guides/adding-a-connector.md`

Do not re-derive the architecture here — open the guide for the trait contract,
the file-by-file walkthrough, and the spec/RFC loop. This skill only lists the
executable steps and the gotchas that bite if you skip them.

## 0. Source of truth

- Connector guide: `docs/guides/adding-a-connector.md` (full narrative).
- Reference connector to copy: `crates/taktora-connector-can` (the minimal
  complete connector — copy and rename it).
- Publish-order guard: `scripts/check-publish-deps.sh`.

## 1. Scaffold the two crate directories

Every connector is **two** crates: the published `taktora-connector-<proto>`
and a `publish = false` `taktora-connector-<proto>-tests`. Create both skeletons
with the bundled helper (idempotent — refuses to clobber existing crates):

```bash
.claude/skills/add-connector/scaffold.sh <proto>
```

Then follow the guide to fill `Cargo.toml`, `routing.rs`, `options.rs`,
`health.rs`, the backend trait + mock/real, the gateway/bridge/registry/
dispatcher, and `connector.rs`.

## 2. The `*-tests` `publish = false` split (do not skip)

The tests crate is mandatory, not optional. Internal `dev-dependencies` on
sibling workspace crates inside a *published* crate break release-plz's publish
topological sort (it ignores dev-dep edges, but `cargo publish`'s verify step
resolves them). Keep every internal dev-dep in the `publish = false`
`*-tests` crate. The guide's Step 12 shows the exact `Cargo.toml`.

## 3. Verify the publish order before you push

After wiring both crates into the workspace, run the guard that encodes the
trap above and fails on a version-bearing internal dev-dep in a publishable
crate:

```bash
./scripts/check-publish-deps.sh
```

This is the same check CI runs. A green run here means the split is correct.

## 4. Build / test / lint at CI's exact flags

```bash
cargo build --workspace
cargo test --workspace --all-features -- --test-threads=1
cargo clippy --workspace --all-targets --all-features -- -D warnings
typos
```

Two gotchas baked into those flags:

- **`--test-threads=1` is required, not a preference.** Tests stand up iceoryx2
  nodes and shared-memory segments and touch a process-wide allocator; running
  them in parallel exhausts `/dev/shm` on constrained runners and cross-talks
  between tests. Serial execution isolates each test's shared-mem resources.
- **Always lint with `--all-features`.** The real protocol backend lives behind
  a default-off `*-integration` feature, so a bare `cargo clippy` never compiles
  it. If the backend is also platform-gated (CAN/EtherCAT are Linux-only),
  `cfg`-gated files escape clippy on macOS entirely — run the clippy line on the
  target platform before merging.

## 5. Spec + traceability

The connector also needs its `feat::`/`req::`/`spec::`/`test::` paperwork and a
`draft -> open` status promotion before code, then `req::` flipped to
`implemented` with `:links:` once it lands. That whole loop, and the
`sphinx-build -W` gate that enforces it, is in the guide (sections 2, 4, 5) —
follow it there.
