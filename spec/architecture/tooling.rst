Workspace tooling — architecture
================================

Design decisions for repo-wide developer tooling (:need:`FEAT_0120`).
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

Implementation footprint
------------------------

.. impl:: Coverage tooling — scripts/coverage.sh + ci-coverage.yml
   :id: IMPL_0092
   :status: open
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

Decisions at a glance
---------------------

.. needtable::
   :types: arch-decision
   :filter: "tooling" in docname
   :columns: id, title, status, refines
