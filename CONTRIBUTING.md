# Contributing to taktora

## The honest disclaimer

taktora is a **pre-1.0 personal experiment, not for production.**
The architecture is sound and the test suite is real, but the API
is not stable, no version has been published, the `unsafe` story
has not been independently audited, and there is no SLA, support,
or backwards-compatibility guarantee.

Contributions are welcome under those terms. PR reviews and issue
responses are best-effort.

## Before you open an issue

- **Bugs and feature requests:** pick the matching template from
  the [issue chooser](https://github.com/patdhlk/taktora/issues/new/choose).
- **Documentation issues:** the `docs:` template covers the README,
  the spec site at <https://taktora.dev/>, per-crate `README.md`,
  and per-crate rustdoc.
- **Substantial designs:** open an RFC issue (`rfc:` template) and
  reference (or draft) a spec under
  `docs/superpowers/specs/YYYY-MM-DD-<topic>-design.md`.
- **Security vulnerabilities:** use the private security-advisory
  flow described in [SECURITY.md](SECURITY.md). Do not file public
  security issues.

## Local development

### Toolchain

- Rust **1.85+** stable, edition **2024**.
- iceoryx2 **0.8.x** is workspace-pinned; no separate install needed.

### Optional system dependencies

These are only required if you want to run the matching integration
tests:

| Dependency | For |
|------------|-----|
| `dlt-daemon` reachable on UDS or TCP | `taktora-log-dlt` system tests (`--features system-tests`). |
| Linux box with SocketCAN (`vcan0` or a real CAN interface) | `taktora-connector-can` real-bus tests (`--features socketcan-integration`). |
| EtherCAT hardware reachable via the NIC named in `ETHERCAT_TEST_NIC` | `taktora-connector-ethercat` real-bus tests (`--features bus-integration`). |
| Running Zenoh router or peer | `taktora-connector-zenoh` real-session tests (`--features zenoh-integration`). |

### Pre-commit hooks

```bash
pre-commit install
pre-commit install --hook-type pre-push
```

Hooks defined in `.pre-commit-config.yaml`. Fast checks run on
every commit; clippy and rustdoc-with-`-D warnings` run on push.

## Building, testing, linting

```bash
# Build the workspace.
cargo build --workspace

# Run the full test matrix. Single-threaded because each test
# creates its own iceoryx2 service in shared memory (parallel
# runs would contend on the same names) and the
# `CountingAllocator` used by the zero-alloc tests is
# process-wide.
cargo test --workspace --all-features -- --test-threads=1

# Lint at the same level CI enforces.
cargo clippy --workspace --all-targets --all-features -- -D warnings

# Spell-check (config in typos.toml).
typos
```

## Coding conventions

- Workspace edition 2024, MSRV 1.85.
- Per-crate clippy lints are already declared in each
  `Cargo.toml`. Do not add `#[allow(...)]` without a
  justifying comment.
- `unsafe`: every block needs a `// SAFETY:` comment naming the
  invariant.
- New connectors: ship a mock back-end alongside the real one,
  gate the real one behind a `*-integration` Cargo feature.
- Tests for cross-crate behavior live in dedicated `*-tests`
  crates (e.g. `taktora-connector-ethercat-tests`).

## Commit & PR conventions

- **[Conventional Commits](https://www.conventionalcommits.org/).**
  `release-plz` parses commit messages to drive versioning and
  changelog generation.
- Examples:
  - `feat(executor): add zero-copy publisher loan path`
  - `fix(connector-ethercat): release SDO lock on PRE-OP rollback`
  - `docs(log-dlt): clarify UDS reconnect semantics`
- **Squash-merge** by default; the squashed title becomes the
  conventional commit on `main`.
- Link the issue you're closing with `Closes #N` in the PR
  description.
- The PR template at `.github/PULL_REQUEST_TEMPLATE.md` walks
  through the rest of the checklist.

## Specs and RFCs

The project's specification is authored with
[sphinx-needs](https://sphinx-needs.readthedocs.io/) and lives in
the `spec/` tree, published to <https://taktora.dev/>. Need types
declared in `spec/ubproject.toml` include `feat::` (features),
`req::` (requirements), `spec::` (specifications), `impl::`
(implementation pointers), `arch-decision::` (ADRs),
`constraint::`, `quality-goal::`, and others.

Anything substantial — new connector, new public trait, behavior
that changes the cycle-loop semantics, anything that needs more
than a paragraph to explain — goes through this loop:

1. Open an RFC issue using `03-rfc.yml`.
2. Draft the design as sphinx-needs directives in the relevant
   `spec/` page:
   - New capabilities → `feat::` + child `req::` in
     `spec/requirements/<topic>.rst`.
   - Architecture / design decisions → `arch-decision::` in
     `spec/architecture/<topic>.rst`.
   - Safety analyses → `risk::` / `tsr::` / `aou::` in
     `spec/safety/<topic>.rst`.
   Set `:status: draft` on every new need.
3. Discuss in the RFC issue, referencing the draft need IDs
   (e.g. `FEAT_0123`, `REQ_0456`, `ADR_0007`).
4. Promote `:status:` from `draft` to `open` once accepted.
5. Open the implementation PR; link the RFC issue and the need
   IDs it satisfies via `:satisfies:` on `impl::` directives.

## Labels

The issue templates auto-apply these labels. They are not
pre-created in the GitHub UI; create them once, then they apply
on every new issue.

| Label | Used by |
|-------|---------|
| `triage` | every issue template |
| `kind:bug` | bug, performance |
| `kind:feature` | feature |
| `kind:rfc` | RFC |
| `area:docs` | docs |
| `area:soundness` | soundness |
| `area:perf` | performance |
| `area:connector-ethercat` | EtherCAT |
| `area:connector-zenoh` | Zenoh |
| `area:connector-can` | CAN |
| `area:log-dlt` | DLT logging |

## License

taktora is dual-licensed under Apache-2.0 OR MIT, at your option
(see `LICENSE-APACHE` and `LICENSE-MIT`). By contributing, you
agree your contribution is licensed under both.
