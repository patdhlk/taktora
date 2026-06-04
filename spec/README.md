# taktora-executor — Specification

Engineering-as-code specification for the [taktora-executor](https://github.com/patdhlk/taktora) crate, authored as a [Sphinx](https://www.sphinx-doc.org) site with [sphinx-needs](https://sphinx-needs.com) directives and the [sphinx-hextra](https://github.com/patdhlk/sphinx-hextra) theme. Tooling-managed via [uv](https://docs.astral.sh/uv/).

> **Personal experiment.** Same warning as the parent project: APIs and requirements may shift, no SLA, fork before relying on it.

## Build

The canonical path uses a project-local uv venv:

```bash
cd spec
uv sync                                    # install deps from pyproject.toml
uv run sphinx-build -b html . _build/html
open _build/html/index.html                # macOS; xdg-open on Linux
```

Equivalent `make` targets:

```bash
make install   # → uv sync
make html      # → uv run sphinx-build -b html . _build/html
make strict    # → uv run sphinx-build -W -b html . _build/html  (CI)
make clean     # → rm -rf _build
```

### Ephemeral build via uvx (no project venv)

If you want to build once without persisting a venv (CI, drive-by readers):

```bash
uvx --with sphinx-needs==8.0.0 \
    --with sphinx-hextra \
    --with myst-parser \
    --from sphinx \
    sphinx-build -W -b html spec spec/_build/html
```

`uvx` runs the tool in a one-shot environment; the `--with` flags add the
extensions Sphinx needs to load. The pyproject.toml-based path (`uv sync` +
`uv run sphinx-build`) is faster after the first invocation.

## Diagram validation (mermaid)

`scripts/validate-mermaid.mjs` parses every `.. mermaid::` block through the
real mermaid renderer to catch syntax errors before deploy. It runs in CI and
as the `mermaid-validate` pre-commit hook. Its npm dependencies (`mermaid`,
`jsdom`) live in the gitignored `scripts/node_modules/`, so install them once
per checkout:

```bash
cd spec/scripts
npm ci                       # installs mermaid + jsdom from package-lock.json
npm run validate-mermaid     # optional; CI and the pre-commit hook run this for you
```

Without this install the hook fails with `Cannot find module 'jsdom'` and blocks
spec commits — a missing-dependency symptom, not a tool bug. For a
**diagram-free** spec commit you may bypass it with
`SKIP=mermaid-validate git commit …`, but running `npm ci` is the proper fix.

## Layout

```
spec/
├── pyproject.toml          # uv-managed deps (sphinx, sphinx-needs, sphinx-hextra, myst-parser)
├── ubproject.toml          # sphinx-needs config (need types + extra link types)
├── conf.py                 # Sphinx config (extensions, theme, needs_from_toml)
├── index.rst               # Master TOC
├── overview.rst            # High-level features (feat directives)
├── requirements/           # System requirements (req directives)
├── architecture/           # Detailed design (spec directives)
├── verification/           # Test cases (test directives)
├── scripts/                # Node tooling — mermaid validator (npm ci to install deps)
└── _static/                # Theme overrides / custom assets
```

## Need types

Declared in `ubproject.toml` and consumed by sphinx-needs at build time:

| Directive | Prefix    | Role |
|-----------|-----------|------|
| `feat`    | `FEAT_`   | User-facing capability |
| `req`     | `REQ_`    | System-level requirement |
| `spec`    | `SPEC_`   | Detailed design / interface |
| `impl`    | `IMPL_`   | Implementation pointer (file/symbol reference) |
| `test`    | `TEST_`   | Verification / test case |

## Link types

Beyond the built-in `:links:` option:

- `satisfies` — `req` → `feat`
- `refines` — `spec` → `req`
- `implements` — `impl` → `spec`
- `verifies` — `test` → `req` (or `spec`)

## Tooling integration

`pharaoh.toml` and `.pharaoh/project/` (after running `/pharaoh:pharaoh-setup`)
configure [Pharaoh](https://github.com/useblocks/pharaoh) workflow gates and
tailoring against the declared types.
