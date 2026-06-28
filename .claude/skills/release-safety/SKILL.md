---
name: release-safety
description: >-
  Use when touching anything that affects crate publishing or releases in the
  taktora workspace — adding a crate, adding a dev-dependency on a sibling
  crate, splitting tests out, or when CI's publish-deps / dep-gating check
  fails (e.g. "check-publish-deps failed", "why does release-plz reorder my
  crate", "where do internal test deps go"). Carries the release-ordering
  gotchas and what to do when the guard trips; points at the scripts and config
  as source of truth.
---

# release-safety

This skill is the **action companion** to the release tooling. The scripts and
config are the source of truth for the exact rules — read them before changing
publishing behaviour:

- Publish-order config: `release-plz.toml` (what release-plz does on `main`).
- The dev-dep publish-ordering guard: `scripts/check-publish-deps.sh` (its
  header documents the trap in full).
- The feature-gating guards: `scripts/check_dep_gating.sh` (zenoh) and
  `scripts/check_dep_gating_can.sh` (socketcan).

Do not re-derive those rules here — open the script header for the precise jq
predicate and the upstream-bug citation. This skill only names the trap, the
actions, and the gotchas that bite if you skip them.

## The core trap: internal dev-deps in published crates

A **published** crate that carries a *version-bearing* dev-dependency on a
sibling `taktora-*` workspace crate breaks release ordering. release-plz
topologically sorts crates for `cargo publish` using only the normal/build
dependency graph — it ignores dev-dep edges (upstream
release-plz#2697 / issue #27). But `cargo publish`'s verify step *does* resolve
dev-deps against the registry. So the dependent can be published *before* the
sibling it dev-depends on; release-plz has just bumped that sibling, rewrites
the dev-dep to the new version, and the index does not have it yet — the
release aborts half-applied.

The edge is only dangerous when **both** hold: there is no parallel normal/build
edge to force the order, and the dev-dep carries a version requirement. A
path-only dev-dep (`req == "*"`) is stripped from the published manifest, so it
is safe.

## How to recognize it

- You added `#[cfg(test)]` code in a *published* crate that pulls in a sibling
  `taktora-*` crate, and wired it as a `[dev-dependencies]` entry with a version.
- `scripts/check-publish-deps.sh` (run locally or in CI) fails, naming the
  `crate -> sibling (dev-dep, req "…", no normal/build edge)` pair.

## The fix: the `publish = false` `*-tests` split

Move the offending test (and its dev-dep) into a sibling `publish = false`
`*-tests` crate. That crate is excluded from the publish graph entirely, so its
dev-dep edges can never reorder a release. The canonical example is
`crates/taktora-executor-tests`; every connector follows the same pattern (e.g.
`crates/taktora-connector-ethercat-tests`).

Two halves make the split complete — do both:

1. The new crate's `Cargo.toml` sets `publish = false`.
2. `release-plz.toml` carries a matching `[[package]]` override with
   `release = false` / `publish = false` (and changelog/tag/release disabled) so
   release-plz emits no version-bump, changelog, tag, or GitHub-Release noise
   for it. Add an entry there for every new `*-tests` crate.

(Alternative: make the dev-dep path-only by dropping its version. The guard
accepts that too, but the `*-tests` split is the established convention.)

## What a contributor does when the check fails

1. Read the failure line — it names the publishable crate and the sibling.
2. Relocate that test into the crate's `*-tests` sibling (create one if absent,
   mirroring `crates/taktora-executor-tests`), or strip the dev-dep's version.
3. Add/confirm the `[[package]]` `release = false` block in `release-plz.toml`
   for any new `*-tests` crate.
4. Re-run `./scripts/check-publish-deps.sh` — a green run means the order is safe.

## Related but distinct: feature-gating guards

`scripts/check_dep_gating.sh` and `scripts/check_dep_gating_can.sh` guard a
*different* property — that the heavy real-backend deps (`zenoh`, `socketcan`)
stay behind their default-off `*-integration` feature and never leak into a
default build. They are not about publish order. If one of those fails, the fix
is in the connector crate's feature wiring, not in `release-plz.toml`.
