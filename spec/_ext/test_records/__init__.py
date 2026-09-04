"""Local Sphinx extension: test-execution records (FEAT_0122, IMPL_0093).

Binds a build to the tests that validated it. Three inputs, no bespoke
parsing (``ADR_0138``):

1. ``sphinx-codelinks`` analysis (used as a library) extracts ``@need-ids:``
   markers from the configured Rust sources into ``(file, fn, TEST_ id)``
   bindings. A marker naming an id that is not an existing ``test`` need is a
   Sphinx warning, so ``sphinx-build -W`` fails the build (``REQ_1012``).
   codelinks itself extracts markers as data only; the enforcement lives here.
2. ``sphinx-test-reports`` ingests the ``cargo-nextest`` JUnit into
   ``test-case`` needs (``REQ_1011``) through the ``test-results-if-present``
   directive below, which degrades to a note when the JUnit is absent so a
   plain docs build stays clean.
3. On ``build-finished`` the bindings are joined to the executed cases on
   test-function identity (``REQ_1013``) and ``test-execution-record.json``
   is written, stamped with the build identity (``REQ_1014``) and carrying
   the coverage-gap list (``REQ_1015``). Without executed cases no record is
   written: a record is validation evidence, never a placeholder.

Configuration (``conf.py``)::

    test_records_sources = {"src_dir": "../crates", "include": ["taktora-medkit-*/**/*.rs"]}
    test_records_marker = "@need-ids:"
    test_records_scope = "verification/medkit"
    test_records_output = "test-execution-record.json"
"""

from __future__ import annotations

import json
import os
import subprocess
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

from docutils import nodes
from sphinx.application import Sphinx
from sphinx.util import logging

from .join import bindings_from_refs, build_record, find_dangling, is_case

logger = logging.getLogger(__name__)
_ATTR = "_test_records_bindings"


# --------------------------------------------------------------------------- codelinks
def _analyse_markers(app: Sphinx) -> list[dict[str, Any]]:
    """Run the codelinks analysis over the configured sources; return raw refs."""
    from sphinx_codelinks.analyse.analyse import SourceAnalyse
    from sphinx_codelinks.config import NeedIdRefsConfig, SourceAnalyseConfig
    from sphinx_codelinks.source_discover.config import CommentType, SourceDiscoverConfig
    from sphinx_codelinks.source_discover.source_discover import SourceDiscover

    class _Analyse(SourceAnalyse):
        """SourceAnalyse minus the git-remote lookups.

        The record needs file paths and scopes, not blob URLs, and the upstream
        constructor warns when ``.git`` is a file (git worktrees) — noise this
        build has no use for.
        """

        def __init__(self, cfg: SourceAnalyseConfig) -> None:  # noqa: D107
            self.name = "test-records"
            self.analyse_config = cfg
            self.src_files, self.src_comments = [], []
            self.need_id_refs, self.oneline_needs, self.marked_rst = [], [], []
            self.all_marked_content = []
            self.git_root = self.git_remote_url = self.git_commit_rev = None
            self.project_path = cfg.src_dir
            self.oneline_warnings = []
            self._flags_map_cache = {}

    src_cfg = app.config.test_records_sources
    src_dir = (Path(app.confdir) / src_cfg["src_dir"]).resolve()
    discover = SourceDiscover(SourceDiscoverConfig(
        src_dir=src_dir, include=list(src_cfg.get("include", [])),
        exclude=list(src_cfg.get("exclude", [])), comment_type="rust",
    ))
    analyse = _Analyse(SourceAnalyseConfig(
        src_files=discover.source_paths, src_dir=src_dir, comment_type=CommentType.rust,
        get_need_id_refs=True, get_oneline_needs=False, get_rst=False,
        need_id_refs_config=NeedIdRefsConfig(markers=[app.config.test_records_marker]),
    ))
    analyse.run()
    refs = []
    for ref in analyse.need_id_refs:
        scope = ref.tagged_scope
        refs.append({
            "filepath": str(ref.filepath),
            "lineno": ref.source_map["start"]["row"] + 1,
            "tagged_scope": scope.text.decode("utf-8") if scope is not None and scope.text else "",
            "need_ids": list(ref.need_ids),
        })
    logger.info("test-records: %d source files, %d markers", len(analyse.src_files), len(refs))
    return refs


def _on_builder_inited(app: Sphinx) -> None:
    setattr(app, _ATTR, bindings_from_refs(_analyse_markers(app)))


def _needs_view(app: Sphinx) -> dict[str, dict[str, Any]]:
    from sphinx_needs.data import SphinxNeedsData

    return {i: dict(n) for i, n in SphinxNeedsData(app.env).get_needs_view().items()}


def _on_check_consistency(app: Sphinx, env: Any) -> None:
    """REQ_1012: a dangling marker fails the strict build."""
    bindings = getattr(app, _ATTR, [])
    for d in find_dangling(bindings, _needs_view(app)):
        logger.warning(
            "@need-ids: marker names %s but %s (%s:%s)",
            d["test_id"], d["reason"], d["filepath"], d["lineno"],
            type="test_records", subtype="dangling",
        )


# --------------------------------------------------------------------------- identity
def _identity(repo_root: Path) -> dict[str, Any]:
    """REQ_0990-shaped build identity of the checkout the spec was built from."""

    def git(*args: str, default: str = "unknown") -> str:
        try:
            return subprocess.check_output(
                ["git", *args], cwd=repo_root, text=True, stderr=subprocess.DEVNULL
            ).strip()
        except (OSError, subprocess.CalledProcessError):
            return default

    porcelain = git("status", "--porcelain", "--untracked-files=no", default="")
    return {
        "git_sha": git("rev-parse", "HEAD"),
        "git_short": git("rev-parse", "--short", "HEAD"),
        "git_describe": git("describe", "--tags", "--always", "--dirty"),
        "git_dirty": bool(porcelain),
        "build_timestamp": os.environ.get(
            "TAKTORA_BUILD_TIMESTAMP",
            datetime.now(UTC).replace(microsecond=0).isoformat().replace("+00:00", "Z"),
        ),
    }


# --------------------------------------------------------------------------- record
def _on_build_finished(app: Sphinx, exc: Exception | None) -> None:
    if exc is not None:
        return
    needs = _needs_view(app)
    if not any(is_case(n) for n in needs.values()):
        logger.info("test-records: no executed test cases ingested — record not written")
        return
    record = build_record(
        getattr(app, _ATTR, []), needs, _identity(Path(app.confdir).parent),
        scope=app.config.test_records_scope,
    )
    out = Path(app.outdir) / app.config.test_records_output
    out.write_text(json.dumps(record, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    s = record["summary"]
    logger.info(
        "test-records: wrote %s (%d validated, %d gaps of %d in scope, commit %s)",
        out.name, s["validated"], sum(s["gaps"].values()), s["tests_in_scope"],
        record["build"]["git_short"],
    )


# --------------------------------------------------------------------------- directive
def _make_directive():
    from sphinxcontrib.test_reports.directives.test_file import TestFileDirective

    class TestResultsIfPresent(TestFileDirective):
        """``test-file`` that degrades to a note when the JUnit is absent.

        The JUnit only exists after the medkit ``cargo-nextest`` leg has run
        (``.github/workflows/ci-test-records.yml``). A plain docs build has no
        results to ingest and must still pass ``-W``, so the directive says so
        instead of letting sphinx-test-reports warn about a missing file.
        """

        def run(self):  # noqa: D102
            given = self.options.get("file", "")
            path = Path(given)
            if not path.is_absolute():
                path = Path(self.env.app.config.tr_rootdir) / path
            if not path.exists():
                note = nodes.note()
                note += nodes.paragraph(
                    text="No executed test results were ingested in this build. "
                    "The medkit test-execution results appear here when the spec is "
                    "built after the cargo-nextest leg (CI test-records workflow).")
                return [note]
            # Act as the configured test-file directive for id/type resolution.
            self.name = self.env.app.config.tr_file[0]
            return super().run()

    return TestResultsIfPresent


def setup(app: Sphinx) -> dict[str, Any]:
    app.setup_extension("sphinxcontrib.test_reports")
    app.add_config_value("test_records_sources", {"src_dir": "../crates", "include": []}, "env", types=[dict])
    app.add_config_value("test_records_marker", "@need-ids:", "env", types=[str])
    app.add_config_value("test_records_scope", "verification", "env", types=[str])
    app.add_config_value("test_records_output", "test-execution-record.json", "env", types=[str])
    app.add_directive("test-results-if-present", _make_directive())
    app.connect("builder-inited", _on_builder_inited)
    app.connect("env-check-consistency", _on_check_consistency)
    app.connect("build-finished", _on_build_finished)
    return {"version": "0.1", "parallel_read_safe": True, "parallel_write_safe": True}
