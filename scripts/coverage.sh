#!/usr/bin/env bash
# Workspace line-coverage measurement via cargo-llvm-cov (ADR_0134).
# Spec: FEAT_0120, REQ_0991..REQ_0996 — spec/requirements/tooling/coverage.rst
#
# One instrumented test run, three reports from the same profile data:
#   terminal summary (stdout)
#   HTML            target/llvm-cov/html/index.html
#   lcov trace      target/llvm-cov/lcov.info
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# REQ_0996: hard-fail with an install hint, unlike the advisory tooling gates —
# a coverage run that silently skips would report nothing to act on.
if ! command -v cargo-llvm-cov >/dev/null 2>&1; then
  echo "coverage: cargo-llvm-cov not found." >&2
  echo "  install: cargo install cargo-llvm-cov --locked" >&2
  exit 1
fi

# REQ_0994: keep generated sources (build-script OUT_DIR output lands under
# target/) and the xtask dev tooling out of the coverage denominator. The
# generator's own coverage is the signal; its output is noise.
IGNORE_RE='(^|/)target/|(^|/)xtask/'

# REQ_0992: --all-features — matches the hermetic all-features CI leg.
# REQ_0993: --test-threads=1 — each Executor builds an iceoryx2 node plus
# shared-memory segments; parallel test processes can exhaust /dev/shm
# (same rationale as the CI test job).
cargo llvm-cov --workspace --all-features --no-report \
  --ignore-filename-regex "$IGNORE_RE" \
  -- --test-threads=1

# REQ_0995: three report formats from the single run above. (`report --lcov`
# does not create the parent directory itself; `report --html` does.)
mkdir -p target/llvm-cov
cargo llvm-cov report --lcov --output-path target/llvm-cov/lcov.info \
  --ignore-filename-regex "$IGNORE_RE"
cargo llvm-cov report --html --ignore-filename-regex "$IGNORE_RE"

# REQ_1001: gate-ready, not gating. With COVERAGE_FAIL_UNDER_LINES set
# (e.g. "85"), the summary report exits nonzero below that line-coverage
# floor. It runs last, so a tripped floor still leaves the lcov + HTML
# reports above for inspection. Unset (the default, and CI today):
# informational only.
FAIL_UNDER=()
if [[ -n "${COVERAGE_FAIL_UNDER_LINES:-}" ]]; then
  FAIL_UNDER=(--fail-under-lines "$COVERAGE_FAIL_UNDER_LINES")
fi
cargo llvm-cov report --ignore-filename-regex "$IGNORE_RE" \
  ${FAIL_UNDER[@]+"${FAIL_UNDER[@]}"} | tee target/llvm-cov/summary.txt

echo
echo "coverage: HTML report at target/llvm-cov/html/index.html"
echo "coverage: lcov trace at target/llvm-cov/lcov.info"
