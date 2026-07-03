Workspace tooling tests
=======================

Verification for the coverage tooling (:need:`FEAT_0120`). Unlike crate
tests, these are procedural verifications of developer tooling: each
describes the check, how it was executed, and the observed evidence.
The CI-facing cases (:need:`TEST_0975`, :need:`TEST_0979`) re-execute on
every code-changing PR via ``.github/workflows/ci-coverage.yml``; the
one-shot cases were executed at introduction (2026-07-03) and are cheap
to repeat locally.

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
