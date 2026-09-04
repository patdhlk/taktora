"""Unit tests for the test-execution-record join (TEST_0984, TEST_0985).

Pure functions over three inputs: codelinks ``@need-ids:`` refs, the
sphinx-needs view (spec ``test`` needs + STR ``test-case`` needs), and the
build identity. Fixtures under ``fixtures/`` pin the shapes.
"""

import json
import pathlib

import pytest

from .join import bindings_from_refs, build_record, find_dangling

FX = pathlib.Path(__file__).parent / "fixtures"
IDENTITY = {
    "git_sha": "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
    "git_short": "deadbee",
    "git_describe": "v0.1.0-1-gdeadbee",
    "git_dirty": False,
    "build_timestamp": "2026-07-05T00:00:00Z",
}


@pytest.fixture
def refs():
    return json.loads((FX / "refs.json").read_text())


@pytest.fixture
def needs():
    return json.loads((FX / "needs.json").read_text())


def test_bindings_resolve_crate_and_fn_from_ref(refs):
    b = bindings_from_refs(refs)
    assert b[0] == {
        "test_id": "TEST_0900",
        "crate": "taktora-medkit-model",
        "fn": "round_trips",
        "filepath": "/repo/crates/taktora-medkit-model/src/lib.rs",
        "lineno": 680,
    }
    # `async fn` scopes resolve too; one binding per (ref, need id)
    assert b[1]["fn"] == "root_advertises_extensions"
    assert b[1]["crate"] == "taktora-medkit-gateway-axum-tests"
    assert len(b) == 5


def test_bindings_ignore_refs_with_no_fn_scope():
    refs = [{"filepath": "/repo/crates/x/src/lib.rs", "lineno": 1,
             "tagged_scope": "struct Foo;", "need_ids": ["TEST_0900"]}]
    assert bindings_from_refs(refs) == []


def test_find_dangling_flags_unknown_ids_and_wrong_types(needs):
    bindings = bindings_from_refs([
        {"filepath": "/repo/crates/a/tests/t.rs", "lineno": 3, "tagged_scope": "fn ok() {}", "need_ids": ["TEST_0900"]},
        {"filepath": "/repo/crates/a/tests/t.rs", "lineno": 9, "tagged_scope": "fn nope() {}", "need_ids": ["TEST_9999"]},
        {"filepath": "/repo/crates/a/tests/t.rs", "lineno": 15, "tagged_scope": "fn wrong() {}", "need_ids": ["REQ_0914"]},
    ])
    dangling = find_dangling(bindings, needs)
    assert [(d["test_id"], d["reason"]) for d in dangling] == [
        ("TEST_9999", "unknown need id"),
        ("REQ_0914", "not a test need (type req)"),
    ]
    assert dangling[0]["filepath"].endswith("tests/t.rs") and dangling[0]["lineno"] == 9


def test_build_record_matches_golden(refs, needs):
    record = build_record(bindings_from_refs(refs), needs, IDENTITY, scope="verification/medkit")
    assert record == json.loads((FX / "expected_record.json").read_text())


def test_gap_reasons_cover_unmarked_failed_and_unexecuted(refs, needs):
    record = build_record(bindings_from_refs(refs), needs, IDENTITY, scope="verification/medkit")
    gaps = {g["test_id"]: g["reason"] for g in record["coverage_gaps"]}
    assert gaps == {"TEST_0903": "failed", "TEST_0904": "unmarked", "TEST_0909": "unexecuted"}
    validated = {v["test_id"] for v in record["validated"]}
    assert validated == {"TEST_0900", "TEST_0936"}
    assert validated.isdisjoint(gaps)


def test_scope_excludes_tests_outside_the_subtree(refs, needs):
    record = build_record(bindings_from_refs(refs), needs, IDENTITY, scope="verification/medkit")
    listed = {v["test_id"] for v in record["validated"]} | {g["test_id"] for g in record["coverage_gaps"]}
    assert "TEST_0100" not in listed
    assert record["summary"]["tests_in_scope"] == 5


def test_rejected_cases_are_out_of_scope_not_gaps(refs, needs):
    """A retired (rejected) verification case is not a coverage gap (TEST_0914 precedent)."""
    record = build_record(bindings_from_refs(refs), needs, IDENTITY, scope="verification/medkit")
    listed = {v["test_id"] for v in record["validated"]} | {g["test_id"] for g in record["coverage_gaps"]}
    assert "TEST_0914" not in listed
    assert record["summary"]["tests_in_scope"] == 5
