Workspace tooling — architecture
================================

Design decisions for repo-wide developer tooling — test-coverage measurement
(:need:`FEAT_0120`), the onboarding golden path (:need:`FEAT_0121`) and
test-execution records (:need:`FEAT_0122`).
Requirements live in :doc:`../requirements/tooling/index`.

Solution strategy
-----------------

.. arch-decision:: Coverage measured with cargo-llvm-cov
   :id: ADR_0134
   :status: accepted
   :refines: FEAT_0120

   **Context.** The workspace has no coverage tooling — no tool, script,
   or CI job. Any choice must run on the pinned stable toolchain
   (``rust-toolchain.toml``), cover all 67 workspace crates in one
   invocation, and produce numbers accurate enough to anchor a future
   verification argument (and, later, CI gates).

   **Decision.** ``cargo-llvm-cov``: LLVM source-based instrumentation —
   the compiler's own coverage counters — driven through one cargo
   subcommand. Workspace-wide in a single run; emits terminal summary,
   HTML, and lcov outputs; works on stable. Considered and rejected:

   * ``cargo-tarpaulin`` — Linux-only, with ptrace-era accuracy problems
     on async and generic code; its newer LLVM engine wraps the same
     mechanism ``cargo-llvm-cov`` uses natively, leaving no advantage.
   * ``grcov`` — aggregates the same LLVM profraw data but requires
     manual ``RUSTFLAGS`` and profile-collection wiring for the same
     result.

   **Consequences.** ✅ Accurate, compiler-grade line coverage on the
   pinned stable toolchain. ✅ lcov output feeds a later CI/Codecov step
   without revisiting the tool choice. ❌ Contributors need an extra
   installed binary (``cargo install cargo-llvm-cov``) that
   ``rust-toolchain.toml`` cannot pin or provision. ❌ Doctests stay
   unmeasured: doctest instrumentation needs nightly, which the
   toolchain pin rules out (see :need:`FEAT_0120` non-goals).

.. arch-decision:: Coverage results stay in-repo — artifacts + job summary, no Codecov
   :id: ADR_0135
   :status: accepted
   :refines: FEAT_0120

   **Context.** With coverage measured in CI (:need:`REQ_0998`), the
   results need somewhere to go, and the setup must be gate-ready without
   being a gate today. Forces: pre-1.0 personal project with no external
   service accounts wired to the repo; nothing consumes a coverage trend
   yet; local and CI runs must produce the same numbers.

   **Decision.** CI runs the identical local entrypoint
   (``scripts/coverage.sh``) and publishes in-repo: the per-crate summary
   to the GitHub job summary, lcov + HTML as workflow artifacts. Gate
   readiness is an environment variable (``COVERAGE_FAIL_UNDER_LINES``,
   :need:`REQ_1001`) — commented out in the workflow, one line to enable.
   Considered and rejected:

   * **Codecov / Coveralls** — external service and token, source-path
     data egress, PR-comment noise; the trend/diff-coverage features they
     add have no consumer yet. Revisit when trend tracking becomes a real
     need; the lcov artifact is exactly their input format, so nothing is
     foreclosed.
   * **Committing reports to the repo or gh-pages** — churn on every run
     for data that is reproducible from any commit.

   **Consequences.** ✅ Zero external dependencies or secrets; local and
   CI runs cannot drift; enabling a floor is a one-line flip. ❌ No
   historical trend line and no per-PR diff-coverage view. ❌ Artifacts
   expire with the repo's retention window (default 90 days) — the
   summary in the job log outlives them only as long as the log does.

.. arch-decision:: Front door is a curated example + guide, not a facade crate
   :id: ADR_0136
   :status: accepted
   :refines: FEAT_0121

   **Context.** The workspace is 41 independently-versioned pre-1.0 library
   crates with no facade; assembling an application means picking five to
   eight of them by hand, and discovery is the first-hour cost. A ``taktora``
   umbrella crate that re-exported a curated subset behind feature flags is
   the obvious convenience — but it couples those independently-versioned
   crates behind one semver surface, so every underlying breaking change
   either bumps the facade or leaks through it. The repository's examples
   also pin *published* crate versions, so a facade would itself have to be
   published before any example could depend on it.

   **Decision.** The front door is a curated golden-path example
   (:need:`REQ_1002`) plus a two-tier assembly guide (:need:`REQ_1004`,
   :need:`REQ_1005`) — additive documentation and one runnable crate, nothing
   new to version. The ``taktora`` facade crate is **deferred** until the
   curated ~7-crate stack proves stable enough to carry a semver surface.
   Considered and rejected:

   * **Publish the facade now** — incurs a standing versioning liability over
     41 pre-1.0 crates whose APIs still churn, plus a publish-ordering
     constraint (the facade must ship before any example uses it), to buy a
     convenience the docs deliver at near-zero cost.
   * **Docs-only, no runnable example** — a crate list without a working
     "this is how the pieces fit" reference does not fix the first-hour
     problem; the golden path has to be executable.

   **Consequences.** ✅ The discovery fix ships immediately with no standing
   liability — nothing new to version, and the underlying crates keep bumping
   independently. ❌ No ``cargo add taktora`` / ``use taktora::prelude::*``
   one-liner; the canonical stack lives in prose and can drift from reality
   unless the example and guide are maintained together.

Implementation footprint
------------------------

.. impl:: Coverage tooling — scripts/coverage.sh + ci-coverage.yml
   :id: IMPL_0092
   :status: implemented
   :refines: REQ_0991, REQ_0992, REQ_0993, REQ_0994, REQ_0995, REQ_0996, REQ_0997, REQ_0998, REQ_0999, REQ_1000, REQ_1001

   The realising artefacts, all in-repo:

   * ``scripts/coverage.sh`` — the single entrypoint: tool-presence
     check with install hint, one instrumented
     ``--workspace --all-features`` run (``--test-threads=1``),
     denominator exclusions via ``--ignore-filename-regex``, lcov +
     HTML + terminal/``summary.txt`` reports, and the optional
     ``COVERAGE_FAIL_UNDER_LINES`` floor evaluated last.
   * ``.github/workflows/ci-coverage.yml`` — topic workflow (shared
     diff classifier) invoking the same script; job summary + artifact
     publication; the floor env commented out (informational today).
   * ``CONTRIBUTING.md`` "Test coverage" section — contributor-facing
     documentation.

Test-execution records — solution strategy
------------------------------------------

.. arch-decision:: Record assembled from the useblocks toolchain, not a bespoke generator
   :id: ADR_0138
   :status: accepted
   :refines: FEAT_0122

   **Context.** The record must join four facts about a commit: which tests
   ran and their results, which verification case (``test`` need) each test
   claims, which requirements those cases ``:verifies:``, and the build
   identity. Three of the four already live in, or can be produced into, the
   sphinx-needs graph the spec build already emits. The question is whether to
   write a standalone tool that parses JUnit + source + ``needs.json`` and
   emits its own schema, or to assemble the record from existing extensions.

   **Decision.** Use the useblocks extensions, whose data model is
   sphinx-needs itself: ``sphinx-test-reports`` ingests the ``cargo-nextest``
   JUnit into ``test-file`` / ``test-case`` needs (result, time);
   ``sphinx-codelinks`` (its analysis library, driven in-process) extracts
   ``@need-ids:`` source markers into a code→need map; a small local Sphinx
   extension (``spec/_ext/test_records``) checks every marker against the need
   graph and fails the strict build on a dangling case id, joins the two on
   test-function identity, and stamps the build identity. The record is then a
   projection of ``needs.json`` — no separate schema, no separate parser.
   The de-risking spike (2026-07-05) established that codelinks extracts
   markers as data only and has no dangling-reference checker of its own, so
   the enforcement half of :need:`REQ_1012` lives in the local extension
   rather than in codelinks. Considered and rejected:

   * **Bespoke Rust/Python generator** — re-implements JUnit parsing, source
     scanning, and need-graph traversal that the two extensions already do,
     and invents a record schema that would drift from ``needs.json``. The
     marker→case binding would be unenforced prose rather than a ``-W``
     build failure.
   * **Encoding the case id in the test name** (``fn test_0904_…``) — removes
     the join but reintroduces per-function naming churn across the suite and
     still leaves the binding unchecked by the build.

   **Consequences.** ✅ The record is native, already-validated sphinx-needs
   data; the marker→case binding is machine-checked at the source. ✅ The
   later slices (release, deployment, SBOM, approval) extend the same graph
   rather than a bespoke format. ❌ Two new Sphinx extensions become spec
   build dependencies. ❌ The join relies on stable test-function identity
   between ``sphinx-codelinks`` and the JUnit; a rename touched in one place
   but not the other surfaces as a coverage gap (:need:`REQ_1015`) rather
   than a silent miss.

.. arch-decision:: Structured results via cargo-nextest JUnit
   :id: ADR_0139
   :status: accepted
   :refines: FEAT_0122

   **Context.** ``sphinx-test-reports`` needs a JUnit (or JSON) result file.
   The workspace runs tests with plain ``cargo test`` (:need:`FEAT_0120`
   coverage aside), which emits no machine-readable per-test result, and the
   toolchain is pinned to stable (``rust-toolchain.toml``).

   **Decision.** Run the medkit tests under ``cargo-nextest``, which emits
   JUnit XML on stable via its ``[profile.ci.junit]`` configuration.
   Considered and rejected:

   * ``cargo test -- --format json -Z unstable-options`` — the built-in
     structured output requires nightly, which the toolchain pin rules out
     (the same constraint that excludes doctest coverage in
     :need:`ADR_0134`).
   * **Parsing libtest's human output** — brittle and version-coupled; a
     non-starter next to a supported machine format.

   **Consequences.** ✅ Stable-toolchain JUnit that ``sphinx-test-reports``
   ingests directly; nextest's per-test isolation also suits the medkit
   crates. ❌ A second test runner enters the repo alongside ``cargo test``
   and ``cargo-llvm-cov``; the medkit CI leg and the nextest binary must be
   installed and pinned. Adopted for the medkit pilot only; workspace-wide
   nextest adoption is a separate decision.

Test-execution records — implementation footprint
-------------------------------------------------

.. impl:: Test-execution record pipeline — nextest + STR + codelinks + join
   :id: IMPL_0093
   :status: implemented
   :refines: REQ_1010, REQ_1011, REQ_1012, REQ_1013, REQ_1014, REQ_1015, REQ_1016

   The realising artefacts:

   * ``.config/nextest.toml`` — a ``ci`` profile emitting JUnit for the
     medkit crates.
   * ``sphinx-test-reports`` + ``sphinx-codelinks`` added to the spec build
     (``pyproject.toml`` dependencies; ``conf.py`` loads the local extension,
     which pulls in ``sphinxcontrib.test_reports`` and drives the codelinks
     analysis library over the medkit sources with the ``@need-ids:`` marker).
   * The local Sphinx extension ``spec/_ext/test_records`` — ``join.py``
     (pure functions: marker→binding, dangling check, record projection,
     unit-tested under ``pytest`` with golden fixtures) and the Sphinx glue
     that fails ``-W`` on a dangling marker, provides the
     ``test-results-if-present`` directive (a ``test-file`` that degrades to
     a note when no JUnit exists, so plain docs builds stay clean), and emits
     ``test-execution-record.json`` on ``build-finished`` —
     build-identity-stamped, with the coverage-gap list.
   * ``spec/verification/medkit/test-results.rst`` — the ingestion page that
     hosts the ``test-case`` needs in CI builds.
   * ``@need-ids:`` markers in the medkit test crates, migrated from the
     existing ``//! TEST_`` documentation tags.
   * CI wiring (the medkit ``cargo-nextest`` leg feeding the spec build) that
     regenerates and uploads the record as a workflow artifact.

Decisions at a glance
---------------------

.. needtable::
   :types: arch-decision
   :filter: "tooling" in docname
   :columns: id, title, status, refines
