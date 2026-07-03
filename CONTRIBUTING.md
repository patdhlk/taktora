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

### Test coverage

```bash
# Measure workspace line coverage (needs cargo-llvm-cov:
# `cargo install cargo-llvm-cov --locked`).
scripts/coverage.sh
```

One instrumented `--all-features` test run (same serial-test discipline
as above), reported three ways: a per-crate summary on the terminal, an
HTML report at `target/llvm-cov/html/index.html`, and an lcov trace at
`target/llvm-cov/lcov.info`. Generated sources and `xtask` are excluded
from the denominator. Spec: `FEAT_0120` / `ADR_0134`.

## Coding conventions

- Workspace edition 2024, MSRV 1.85.
- Per-crate clippy lints are already declared in each
  `Cargo.toml`. Do not add `#[allow(...)]` without a
  justifying comment.
- `unsafe`: every block needs a `// SAFETY:` comment naming the
  invariant.
- New connectors: ship a mock back-end alongside the real one,
  gate the real one behind a `*-integration` Cargo feature. See the
  step-by-step [Adding a new connector](docs/guides/adding-a-connector.md)
  guide for the full recipe (spec loop, crate scaffold, the `Connector`
  contract, tests, and the build/lint gate).
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

## Agent tooling

The repo carries a small set of project-local [Claude Code](https://docs.claude.com/en/docs/claude-code)
skills and subagents under `.claude/`, committed alongside the code to
assist taktora development. They are **optional dev aids** — nothing in the
build, test, or release path depends on them.

**Hard invariant — keep additions bare-clone-safe.** A committed skill or
agent may depend only on (a) the contents of this repository and (b) the
stock Claude Code tool set (`Read`, `Grep`, `Glob`, `Bash`, `WebFetch`,
etc.). Never assume a globally-installed plugin or skill (Pharaoh,
Superpowers, or any other) is present — everything here must work on a bare
clone with nothing else installed. `scripts/check-tooling-pointers.sh`
(wired into pre-commit and CI) guards this from the file-pointer side: it
verifies that every repo path these `.claude/` files reference still
exists, so a moved guide or renamed script trips the check instead of
silently rotting.

### Agents

Read-only domain advisors: they explain, locate, and review diffs against
the spec, but never edit files (their tool grant is read/search/fetch only).

| Agent | Invoke when… |
|-------|--------------|
| `taktora-ethercat` | you have an EtherCAT question — the connector, ESI device descriptions and the ESI→driver codegen, netcfg / Sync-Manager / PDO assignment, distributed clocks, or bus bring-up topology. |
| `taktora-can` | you have a CAN transport-layer question — the CAN connector, raw frame transport (the bytes on the wire), the CANopen / CiA 402 drive profile, or DBC-driven signal layout. |
| `taktora-j1939` | you have a J1939 application-protocol question — PGN/SPN decoding, address claim, or the multi-frame transport protocol (BAM, ETP, RTS-CTS) layered over CAN. |
| `taktora-realtime` | you have a timing question — cycle jitter, lateness, dispatch drift, executor dispatch modes, hot-path allocation discipline, or SCHED_FIFO / PREEMPT_RT scheduling. |
| `taktora-safety` | you have a functional-safety question — hazards and the HARA, safety goals, the safety concept (FSR/TSR), Freedom From Interference, fail-safe behaviour, or SM-watchdog / FTTI rationale. |
| `taktora-security` | you have a security question — supply-chain / publish integrity, the connector process boundary and fault isolation, or the security impact of a diff. |
| `taktora-aspice` | you have a process-assurance question — requirement-to-implementation-to-verification traceability, V-model coverage, orphan/gap detection, or lifecycle/status gates in the sphinx-needs spec. |

### Skills

| Skill | Use when… |
|-------|-----------|
| `add-connector` | adding a new protocol connector to the `taktora-connector` framework; the action companion to [Adding a new connector](docs/guides/adding-a-connector.md), carrying the mechanical steps and gotchas. |
| `release-safety` | touching anything that affects crate publishing — adding a crate, adding a sibling dev-dependency, splitting tests out, or when CI's `check-publish-deps` guard trips. |
| `pi-bringup` | bringing up a real EtherCAT bus on a Raspberry Pi (or any Linux host), or debugging a Pi bring-up that hangs, runs out of memory, or is denied raw-socket access. |

### Adding a new one

Copy an existing agent or skill as the mold, match its frontmatter and tone,
and keep it bare-clone-safe (repo contents + stock tools only). When it
references a repo path, expect `scripts/check-tooling-pointers.sh` to verify
that path on every commit.

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
