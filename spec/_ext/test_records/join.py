"""Join ``@need-ids:`` source markers to executed test cases → record + gaps.

Pure functions, no I/O. Inputs are the data the two useblocks extensions
already produced (``ADR_0138``):

* *refs* — ``sphinx-codelinks`` need-id references: one dict per marker with
  ``filepath``, ``lineno``, ``tagged_scope`` (the source text of the item the
  marker is attached to) and ``need_ids``.
* *needs* — the sphinx-needs view as a mapping ``id -> need dict``. Spec
  verification cases are ``type == "test"``; ``sphinx-test-reports`` ingests
  the ``cargo-nextest`` JUnit as ``type == "test-case"`` needs carrying
  ``case_name`` / ``classname`` / ``result`` / ``time``.
* *identity* — the build-identity fields of ``REQ_0990``.

Realises ``IMPL_0093`` for ``REQ_1013`` (join), ``REQ_1014`` (record
projection) and ``REQ_1015`` (coverage-gap enumeration); ``find_dangling``
is the build-enforcement half of ``REQ_1012``.
"""

from __future__ import annotations

import re
from collections import defaultdict
from typing import Any

_FN = re.compile(r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)")

# sphinx-test-reports registers the *directive* as ``test-case`` but the need
# type it stores is the second element of ``tr_case`` (``testcase``). Accept both.
CASE_TYPES = frozenset({"test-case", "testcase"})

Need = dict[str, Any]
Needs = dict[str, Need]


def is_case(need: Need) -> bool:
    """True for an executed-test-case need ingested by sphinx-test-reports."""
    return need.get("type") in CASE_TYPES


def _crate_of_path(filepath: str) -> str | None:
    """``…/crates/<crate>/…`` → ``<crate>``; ``None`` when not under ``crates/``."""
    parts = filepath.replace("\\", "/").split("/")
    try:
        return parts[parts.index("crates") + 1]
    except (ValueError, IndexError):
        return None


def _crate_of_classname(classname: str) -> str:
    """nextest JUnit ``classname`` is ``<crate>`` (lib unit tests) or ``<crate>::<test-binary>``."""
    return (classname or "").split("::", 1)[0]


def _fn_of_case(case_name: str) -> str:
    """``tests::foo`` / ``view::tests::foo`` → ``foo``; ``foo`` → ``foo``."""
    return (case_name or "").rsplit("::", 1)[-1]


def bindings_from_refs(refs: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """One binding per ``(marker, need id)`` whose tagged scope is a ``fn`` item.

    Markers whose scope is not a function (a struct, a module) carry no
    test-function identity to join on and are dropped here; the dangling
    check still sees them through ``find_dangling`` only if they resolved.
    """
    out: list[dict[str, Any]] = []
    for ref in refs:
        m = _FN.search(ref.get("tagged_scope") or "")
        crate = _crate_of_path(ref.get("filepath", ""))
        if not m or crate is None:
            continue
        for need_id in ref.get("need_ids", []):
            out.append({
                "test_id": need_id,
                "crate": crate,
                "fn": m.group(1),
                "filepath": ref["filepath"],
                "lineno": ref.get("lineno"),
            })
    return out


def find_dangling(bindings: list[dict[str, Any]], needs: Needs) -> list[dict[str, Any]]:
    """Markers naming an id that is not an existing ``test`` need (``REQ_1012``)."""
    dangling = []
    for b in bindings:
        need = needs.get(b["test_id"])
        if need is None:
            reason = "unknown need id"
        elif need.get("type") != "test":
            reason = f"not a test need (type {need.get('type')})"
        else:
            continue
        dangling.append({**b, "reason": reason})
    return dangling


def _case_row(case: Need) -> dict[str, Any]:
    return {
        "crate": _crate_of_classname(case.get("classname", "")),
        "case": case.get("case_name") or case.get("case") or "",
        "result": case.get("result"),
        "time": case.get("time"),
    }


def build_record(
    bindings: list[dict[str, Any]],
    needs: Needs,
    identity: dict[str, Any],
    *,
    scope: str,
) -> dict[str, Any]:
    """Project validated cases and coverage gaps for every ``test`` need under *scope*.

    Needs with ``status: rejected`` (retired cases) are out of scope.
    A verification case is *validated* when at least one marked test case
    executed for it and every executed case passed. Otherwise it is a gap:

    * ``unmarked`` — no marker anywhere names it;
    * ``unexecuted`` — marked, but no executed case matched the marked
      function (a rename touched in one place, or the crate was not run);
    * ``failed`` — an executed case for it did not pass.
    """
    cases_by_key: dict[tuple[str, str], list[Need]] = defaultdict(list)
    for need in needs.values():
        if is_case(need):
            key = (_crate_of_classname(need.get("classname", "")),
                   _fn_of_case(need.get("case_name") or need.get("case") or ""))
            cases_by_key[key].append(need)

    marked: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for b in bindings:
        marked[b["test_id"]].append(b)

    # Retired cases (status rejected) are not validation targets: listing them
    # as gaps would misreport a deliberate retirement as missing coverage.
    in_scope = sorted(
        i for i, n in needs.items()
        if n.get("type") == "test"
        and n.get("status") != "rejected"
        and (n.get("docname") or "").startswith(scope)
    )

    validated, gaps = [], []
    gap_counts: dict[str, int] = defaultdict(int)
    for test_id in in_scope:
        need = needs[test_id]
        verifies = list(need.get("verifies") or need.get("links") or [])
        cases: list[Need] = []
        seen: set[str] = set()
        for b in marked.get(test_id, []):
            for c in cases_by_key.get((b["crate"], b["fn"]), []):
                if c["id"] not in seen:
                    seen.add(c["id"]); cases.append(c)
        rows = sorted((_case_row(c) for c in cases), key=lambda r: (r["crate"], r["case"]))
        if test_id not in marked:
            reason = "unmarked"
        elif not rows:
            reason = "unexecuted"
        elif any(r["result"] != "passed" for r in rows):
            reason = "failed"
        else:
            validated.append({"test_id": test_id, "verifies": verifies, "cases": rows})
            continue
        gap_counts[reason] += 1
        gaps.append({"test_id": test_id, "verifies": verifies, "reason": reason, "cases": rows})

    executed = sum(1 for n in needs.values() if is_case(n))
    return {
        "build": dict(identity),
        "scope": scope,
        "summary": {
            "tests_in_scope": len(in_scope),
            "validated": len(validated),
            "gaps": dict(sorted(gap_counts.items())),
            "cases_executed": executed,
        },
        "validated": validated,
        "coverage_gaps": gaps,
    }
