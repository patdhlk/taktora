Workspace test-coverage measurement
===================================

Standing tooling that measures line coverage across the workspace, so
coverage is a repeatable verification artifact rather than a one-off number.

.. feat:: Workspace test-coverage measurement
   :id: FEAT_0120
   :status: draft

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
   terminal, HTML, and lcov outputs; contributor documentation.

   **Non-goals (deferred until a baseline exists).** CI integration
   (runner choice, artifact upload, Codecov); coverage thresholds or
   gates; doctest coverage (requires nightly ``-Z doctest-in-workspace``
   instrumentation; the repo pins stable); per-crate coverage targets for
   the safety-relevant core (executor, connector) — a later spec question.

.. req:: Coverage entrypoint script
   :id: REQ_0991
   :status: draft
   :satisfies: FEAT_0120

   The repository shall provide ``scripts/coverage.sh`` that measures line
   coverage across all workspace crates using ``cargo-llvm-cov``.

.. req:: All-features instrumentation
   :id: REQ_0992
   :status: draft
   :satisfies: FEAT_0120

   The coverage run shall compile and test the workspace with
   ``--all-features``, so feature-gated code is instrumented and the
   number is comparable to the CI all-features leg.

.. req:: Serial test execution
   :id: REQ_0993
   :status: draft
   :satisfies: FEAT_0120

   The coverage run shall pass ``--test-threads=1`` to test binaries,
   mirroring CI: each Executor builds an iceoryx2 node plus shared-memory
   segments, and parallel test processes can exhaust ``/dev/shm``.

.. req:: Denominator excludes generated code and dev tooling
   :id: REQ_0994
   :status: draft
   :satisfies: FEAT_0120

   The coverage report shall exclude build-script-generated sources
   (``OUT_DIR`` output) and the ``xtask`` crate from the coverage
   denominator. Coverage of generated output is noise — the generator's
   own coverage is the meaningful signal.

.. req:: Report outputs
   :id: REQ_0995
   :status: draft
   :satisfies: FEAT_0120

   The script shall emit a per-crate terminal summary, an HTML report,
   and an lcov trace file, all under ``target/`` (build artifacts, never
   committed).

.. req:: Missing-tool diagnostic
   :id: REQ_0996
   :status: draft
   :satisfies: FEAT_0120

   When ``cargo-llvm-cov`` is not installed, the script shall exit
   nonzero with an actionable install hint instead of failing mid-run.

.. req:: Contributor documentation
   :id: REQ_0997
   :status: draft
   :satisfies: FEAT_0120

   ``CONTRIBUTING.md`` shall document the coverage entrypoint, what it
   measures, and where the reports land.
