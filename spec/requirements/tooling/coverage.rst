Workspace test-coverage measurement
===================================

Standing tooling that measures line coverage across the workspace, so
coverage is a repeatable verification artifact rather than a one-off number.

.. feat:: Workspace test-coverage measurement
   :id: FEAT_0120
   :status: open

   **Motivation.** No coverage measurement exists in the repository today —
   no tool, no script, no CI job. For a project building a safety argument,
   test coverage is a standing verification artifact: it must be cheap to
   re-measure and produce comparable numbers run over run, which means the
   invocation (feature set, exclusions, test-thread discipline) has to be
   pinned in tooling, not folklore.

   **Scope.** A repeatable local entrypoint (``scripts/coverage.sh``)
   wrapping ``cargo-llvm-cov`` (LLVM source-based instrumentation, runs on
   the pinned stable toolchain); a single ``--all-features`` measurement
   run; denominator curation (generated code and dev tooling excluded);
   terminal, HTML, and lcov outputs; contributor documentation. An
   informational CI job (``ci-coverage.yml``) runs the same entrypoint
   and publishes the results in-repo (:need:`ADR_0135`); the gate
   mechanism exists but no floor is enforced (:need:`REQ_1001`).

   **Non-goals.** Enforcing a coverage floor (the mechanism is prepared,
   the number is a later decision against the baseline); coverage trend
   tracking / external services (Codecov — :need:`ADR_0135`); doctest
   coverage (requires nightly ``-Z doctest-in-workspace``
   instrumentation; the repo pins stable); per-crate coverage targets for
   the safety-relevant core (executor, connector) — a later spec question.

.. req:: Coverage entrypoint script
   :id: REQ_0991
   :status: implemented
   :satisfies: FEAT_0120
   :links: IMPL_0092, TEST_0975

   The repository shall provide ``scripts/coverage.sh`` that measures line
   coverage across all workspace crates using ``cargo-llvm-cov``.

.. req:: All-features instrumentation
   :id: REQ_0992
   :status: implemented
   :satisfies: FEAT_0120
   :links: IMPL_0092, TEST_0975

   The coverage run shall compile and test the workspace with
   ``--all-features``, so feature-gated code is instrumented and the
   number is comparable to the CI all-features leg.

.. req:: Serial test execution
   :id: REQ_0993
   :status: implemented
   :satisfies: FEAT_0120
   :links: IMPL_0092, TEST_0975

   The coverage run shall pass ``--test-threads=1`` to test binaries,
   mirroring CI: each Executor builds an iceoryx2 node plus shared-memory
   segments, and parallel test processes can exhaust ``/dev/shm``.

.. req:: Denominator excludes generated code and dev tooling
   :id: REQ_0994
   :status: implemented
   :satisfies: FEAT_0120
   :links: IMPL_0092, TEST_0976

   The coverage report shall exclude build-script-generated sources
   (``OUT_DIR`` output) and the ``xtask`` crate from the coverage
   denominator. Coverage of generated output is noise — the generator's
   own coverage is the meaningful signal.

.. req:: Report outputs
   :id: REQ_0995
   :status: implemented
   :satisfies: FEAT_0120
   :links: IMPL_0092, TEST_0975

   The script shall emit a per-crate terminal summary, an HTML report,
   and an lcov trace file, all under ``target/`` (build artifacts, never
   committed).

.. req:: Missing-tool diagnostic
   :id: REQ_0996
   :status: implemented
   :satisfies: FEAT_0120
   :links: IMPL_0092, TEST_0977

   When ``cargo-llvm-cov`` is not installed, the script shall exit
   nonzero with an actionable install hint instead of failing mid-run.

.. req:: Contributor documentation
   :id: REQ_0997
   :status: implemented
   :satisfies: FEAT_0120
   :links: IMPL_0092, TEST_0978

   ``CONTRIBUTING.md`` shall document the coverage entrypoint, what it
   measures, and where the reports land.

.. req:: CI measures coverage via the same entrypoint
   :id: REQ_0998
   :status: implemented
   :satisfies: FEAT_0120
   :links: IMPL_0092, TEST_0979

   CI shall measure workspace coverage on code-changing pull requests and
   pushes to ``main`` by running the same entrypoint as local development
   (``scripts/coverage.sh``), so CI and local numbers cannot drift.

.. req:: CI publishes lcov and HTML as workflow artifacts
   :id: REQ_0999
   :status: implemented
   :satisfies: FEAT_0120
   :links: IMPL_0092, TEST_0979

   The CI coverage job shall upload the lcov trace and the HTML report as
   workflow artifacts, including when a (future) coverage floor fails the
   job — the reports that explain a failure must survive it.

.. req:: CI publishes the per-crate summary to the job summary
   :id: REQ_1000
   :status: implemented
   :satisfies: FEAT_0120
   :links: IMPL_0092, TEST_0979

   The CI coverage job shall write the per-crate coverage summary to the
   GitHub job summary, so the numbers are readable without downloading
   artifacts.

.. req:: Optional line-coverage floor, unset by default
   :id: REQ_1001
   :status: implemented
   :satisfies: FEAT_0120
   :links: IMPL_0092, TEST_0980

   The coverage entrypoint shall support an optional line-coverage floor
   via the ``COVERAGE_FAIL_UNDER_LINES`` environment variable, exiting
   nonzero when total line coverage falls below it. The variable shall be
   unset by default and in CI: today the measurement is informational,
   and enabling the gate is a one-line change, not a redesign.
