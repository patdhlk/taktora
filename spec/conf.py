"""Sphinx configuration for the taktora architecture & specification site."""

import json
import sys
from pathlib import Path

# Local Sphinx extensions live under _ext/ (see `test_records` below).
sys.path.insert(0, str(Path(__file__).parent / "_ext"))

# -- Project information -------------------------------------------------------

project = "taktora — Architecture & Specification"
author = "Patrick Dahlke"
copyright = "2026, Patrick Dahlke"
release = "0.1.0"

# -- General configuration -----------------------------------------------------

extensions = [
    "myst_parser",
    "sphinx_needs",
    "sphinx_hextra",
    "sphinxcontrib.mermaid",
    # Test-execution records (FEAT_0122, ADR_0138): loads sphinx-test-reports
    # itself and drives the sphinx-codelinks analysis library — see
    # _ext/test_records/__init__.py and the `test_records_*` values below.
    "test_records",
]

templates_path = ["_templates"]
exclude_patterns = [
    "_build",
    "Thumbs.db",
    ".DS_Store",
    ".venv",
    "README.md",
    ".pharaoh",
    # pytest drops a cache (with a README.md) next to the local extension tests
    # under _ext/; neither it nor the extension sources are spec content.
    ".pytest_cache",
    "_ext",
    # `scripts/` hosts the Node.js mermaid validator (validate-mermaid.mjs)
    # and its npm dependency tree. None of it is spec content; MyST would
    # otherwise parse every README.md / .md file under scripts/node_modules
    # and emit thousands of xref-missing warnings. The validator is invoked
    # by CI directly via `npm run validate-mermaid`, not through Sphinx.
    "scripts",
]

# Allow .rst and .md side by side.
source_suffix = {
    ".rst": "restructuredtext",
    ".md": "markdown",
}

# Treat warnings as errors when invoked with -W (CI uses this).
nitpicky = False

# -- sphinx-needs configuration -----------------------------------------------

# Read need types and link types from ubproject.toml. Keeps directive/prefix/
# link-type declarations out of conf.py so tooling (pharaoh, ubc) can consume
# them as data without parsing Python.
needs_from_toml = "ubproject.toml"

# Schema validation (sphinx-needs schema docs:
# https://sphinx-needs.readthedocs.io/en/latest/schema/index.html). Rules live
# in spec/schemas.json so they're editable as data. `severity: violation` is
# the default and is caught by `sphinx-build -W` in CI. See spec/schemas.json
# for the rule set; current scope is ID format per type, status enum, and
# status=implemented network contracts.
needs_schema_validation_enabled = True
with (Path(__file__).parent / "schemas.json").open("r", encoding="utf-8") as _fh:
    needs_schema_definitions = json.load(_fh)

# -- Test-execution records (FEAT_0122) ----------------------------------------

# Medkit pilot: markers are extracted from the medkit crates only, the record
# projects the medkit verification subtree, and the JUnit (when present) is
# ingested by verification/medkit/test-results.rst. Paths are relative to
# this directory. `tr_rootdir` (sphinx-test-reports) keeps its default — this
# directory — so the `:file:` option on the ingestion page is relative to it.
test_records_sources = {
    "src_dir": "../crates",
    "include": ["taktora-medkit-*/**/*.rs"],
}
test_records_marker = "@need-ids:"
test_records_scope = "verification/medkit"
test_records_output = "test-execution-record.json"

# -- HTML output (sphinx-hextra theme) -----------------------------------------

html_theme = "sphinx_hextra"
html_static_path = ["_static"]
html_title = project

# Override theme + sphinx-needs CSS variables to match the taktora.eu palette.
html_css_files = ["taktora.css"]

# Mark light mode explicitly on <html> so mermaid's theme detector doesn't
# fall through to prefers-color-scheme when the OS is dark.
html_js_files = ["taktora-theme-sync.js"]

# Copy CNAME verbatim into the build output so GitHub Pages serves the apex domain.
html_extra_path = ["CNAME"]

# Canonical URL for the published site (GitHub Pages → taktora.dev).
# Affects only metadata (sitemaps, canonical links); does not change asset paths.
html_baseurl = "https://taktora.dev/"
