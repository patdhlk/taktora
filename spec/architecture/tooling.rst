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

Decisions at a glance
---------------------

.. needtable::
   :types: arch-decision
   :filter: "tooling" in docname
   :columns: id, title, status, refines
