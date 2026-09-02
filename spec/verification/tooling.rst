Workspace tooling tests
=======================

Verification for the workspace tooling — test-coverage measurement
(:need:`FEAT_0120`) and test-execution records (:need:`FEAT_0122`). Unlike
crate tests, these are procedural verifications of developer tooling: each
describes the check, how it was executed, and the observed evidence.
The CI-facing cases (:need:`TEST_0975`, :need:`TEST_0979`) re-execute on
every code-changing PR via ``.github/workflows/ci-coverage.yml``; the
one-shot cases were executed at introduction (2026-07-03) and are cheap
to repeat locally.

Coverage
--------

.. test:: Full coverage run produces the three reports
   :id: TEST_0975
   :status: implemented
   :verifies: REQ_0991, REQ_0992, REQ_0993, REQ_0995

   ``scripts/coverage.sh`` runs end-to-end: one ``cargo llvm-cov``
   invocation with ``--workspace --all-features`` and
   ``-- --test-threads=1``, followed by lcov, HTML, and terminal-summary
   reports from the same profile data. Verified by the baseline run
   (2026-07-03: 87.7% line coverage, all tests passing, all three
   reports present under ``target/llvm-cov/``) and re-executed by every
   ``ci-coverage.yml`` run — first CI evidence: actions run
   ``28669729394`` (PR #182), which uploaded the 2.2 MB
   ``coverage-report`` artifact.

.. test:: Denominator excludes generated code and xtask
   :id: TEST_0976
   :status: implemented
   :verifies: REQ_0994

   Inspection of the per-crate summary (``target/llvm-cov/summary.txt``)
   after a full run: no rows under ``target/`` (``OUT_DIR`` codegen
   output) and no ``xtask`` rows appear; 49 of the 67 workspace crates
   carry measurable source. Repeatable as
   ``grep -E '(^|/)(target|xtask)/' target/llvm-cov/summary.txt``
   expecting no matches.

.. test:: Missing-tool diagnostic
   :id: TEST_0977
   :status: implemented
   :verifies: REQ_0996

   With ``cargo-llvm-cov`` absent from ``PATH``, ``scripts/coverage.sh``
   exits ``1`` before any build and prints the install hint
   (``cargo install cargo-llvm-cov --locked``). Executed 2026-07-03 on a
   machine without the tool installed, prior to installing it.

.. test:: Contributor documentation present
   :id: TEST_0978
   :status: implemented
   :verifies: REQ_0997

   Doc inspection: ``CONTRIBUTING.md`` carries a "Test coverage" section
   under "Building, testing, linting" naming the entrypoint, the install
   prerequisite, what is measured (all-features, exclusions), the three
   report locations, and the CI behaviour.

.. test:: CI measures and publishes on code-changing PRs
   :id: TEST_0979
   :status: implemented
   :verifies: REQ_0998, REQ_0999, REQ_1000

   ``ci-coverage.yml`` gates on the shared diff classifier, runs
   ``scripts/coverage.sh`` (the identical local entrypoint), appends the
   per-crate summary to the GitHub job summary, and uploads
   ``lcov.info`` + the HTML report as the ``coverage-report`` artifact
   with ``if: !cancelled()`` so a failing job still publishes. First
   evidence: actions run ``28669729394`` on PR #182 (success, artifact
   uploaded, summary rendered); re-verified by every subsequent run.

.. test:: Coverage floor semantics (gate-ready, unset by default)
   :id: TEST_0980
   :status: implemented
   :verifies: REQ_1001

   Against real baseline profile data (87.7% lines):
   ``COVERAGE_FAIL_UNDER_LINES`` unset → exit ``0`` and
   ``summary.txt`` written; ``=99`` → exit ``1`` with the lcov and HTML
   reports still intact (the floor check runs last); ``=50`` → exit
   ``0``. Executed 2026-07-03. The workflow ships the variable commented
   out, so CI today is informational only.

Test-execution records
----------------------

.. test:: Nextest emits JUnit for the medkit crates
   :id: TEST_0981
   :status: implemented
   :verifies: REQ_1010

   ``cargo nextest run`` over the medkit crates, under the ``ci`` profile,
   produces a JUnit XML file whose ``<testcase>`` entries carry each medkit
   test's name, classname, result, and time. Verified by running the profile
   and asserting the JUnit exists and lists the known medkit tests.

   Executed 2026-09-02: ``cargo nextest run --profile ci`` over the eight
   medkit crates → 119 passed, ``target/nextest/ci/junit.xml`` with 119
   ``<testcase>`` entries (``name`` carries the module path for unit tests,
   e.g. ``tests::health_rolls_up_worst_wins``; ``classname`` is
   ``<crate>`` or ``<crate>::<test-binary>``). Re-executed by
   ``.github/workflows/ci-test-records.yml`` on every code-changing PR.

.. test:: sphinx-test-reports ingests results as needs
   :id: TEST_0982
   :status: implemented
   :verifies: REQ_1011

   Building the spec against a fixture JUnit yields ``test-file`` and
   ``test-case`` needs in ``needs.json`` whose ``result`` and ``time`` fields
   match the fixture — a passed case reads ``result: passed``, a failed case
   ``result: failed``. Verified against a committed fixture with a known
   pass/fail mix.

   Executed 2026-09-02: the committed fixture
   ``spec/_ext/test_records/fixtures/needs.json`` pins the ingested shape
   (need type ``testcase`` — sphinx-test-reports' stored type for the
   ``test-case`` directive — with ``case_name`` / ``classname`` / ``result``
   / ``time``; ``TC_4`` is the ``failure`` case) and the join tests consume
   it; the real strict build over the nextest JUnit ingested 119 cases under
   ``TF_MEDKIT`` on ``verification/medkit/test-results``.

.. test:: Marker binding is machine-checked at the source
   :id: TEST_0983
   :status: implemented
   :verifies: REQ_1012

   A medkit test carrying ``// @need-ids: TEST_0904`` is extracted by
   ``sphinx-codelinks`` and bound to that verification case. A marker naming a
   non-existent case id (``// @need-ids: TEST_9999``) fails
   ``sphinx-build -W`` — verified by a negative build over a fixture that
   must error, so the enforcement itself is covered, not assumed.

   Executed 2026-09-02: 77 markers over 49 medkit source files resolve to
   ``fn`` scopes (codelinks analysis, no dangling ids). Negative build: a
   probe ``crates/taktora-medkit-model/tests/zz_dangling_probe.rs`` carrying
   ``// @need-ids: TEST_9999`` made ``sphinx-build -W`` exit 1 with
   ``WARNING: @need-ids: marker names TEST_9999 but unknown need id
   (…/zz_dangling_probe.rs:4) [test_records.dangling]``; deleting the probe
   restored a clean build. The detector is also unit-tested
   (``test_find_dangling_flags_unknown_ids_and_wrong_types``: unknown id and
   wrong need type).

.. test:: Executed test joins to its verification case
   :id: TEST_0984
   :status: implemented
   :verifies: REQ_1013

   Over a fixture pairing a marked test with its JUnit result, the
   join extension links the ingested ``test-case`` need to the ``test``
   need named by the marker, matched on test-function identity. Verified by
   asserting the link resolves and that a passing ``test-case`` reaches the
   requirement its case ``:verifies:``.

   Executed 2026-09-02: ``uv run --group dev pytest _ext/test_records`` —
   ``test_bindings_resolve_crate_and_fn_from_ref`` (crate from the
   ``crates/<crate>/`` path, fn from the tagged scope, ``async fn`` included)
   and ``test_build_record_matches_golden`` (``TEST_0900`` joined to
   ``tests::round_trips`` in ``taktora-medkit-model`` and projected with its
   ``verifies`` list). 7 passed.

.. test:: Record is build-stamped and reports coverage gaps
   :id: TEST_0985
   :status: implemented
   :verifies: REQ_1014, REQ_1015

   The emitted ``test-execution-record.json`` carries the build-identity
   fields (:need:`REQ_0990`) from the checkout and projects the executed
   medkit tests, their results, the cases exercised, and the requirements
   validated. A medkit ``test`` need with no joined passing ``test-case``
   appears in the record's coverage-gap list. Verified by a golden-file
   comparison over a fixture with a deliberate unmarked case.

   Executed 2026-09-02: golden ``fixtures/expected_record.json`` covers a
   validated case, a multi-case failure, an unmarked case, a marked-but-
   unexecuted case (renamed test), and a rejected case kept out of scope
   (``test_build_record_matches_golden``,
   ``test_gap_reasons_cover_unmarked_failed_and_unexecuted``,
   ``test_rejected_cases_are_out_of_scope_not_gaps``). The real strict
   build over the nextest JUnit wrote a record stamped ``git_sha`` /
   ``git_short`` / ``git_describe`` / ``git_dirty`` / ``build_timestamp``
   for the checkout and reported 45 validated, 2 gaps (``TEST_0901`` and
   ``TEST_0904``, both ``unmarked`` — no test exists for either) of 47
   medkit cases in scope, from 119 executed cases.

.. test:: CI regenerates and publishes the record
   :id: TEST_0986
   :status: open
   :verifies: REQ_1016

   On a code-changing pull request, CI runs the medkit ``cargo-nextest`` leg
   and the spec build, then uploads ``test-execution-record.json`` as a
   workflow artifact. Verified by the first CI run once the pipeline lands
   (evidence: actions run id + PR number, recorded here on introduction).
