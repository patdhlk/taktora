#!/usr/bin/env bash
# Guard against duplicate sphinx-needs IDs across the spec source.
#
# Every need (REQ_/ADR_/BB_/TEST_/... directive) declares a globally unique
# `:id:`. The IDs are allocated from a single global per-type counter, so two
# spec *areas* developed in parallel can independently grab the same next ID
# and collide on merge — which makes the strict `sphinx-build -W` fail deep in
# the build with a duplicate-ID error (see #156, the third recurrence). This
# lint catches the collision in seconds: it scans the committed spec source for
# `:id:` declarations and exits non-zero, listing every ID defined more than
# once and the files that define it.
#
# Used by .github/workflows/ci.yml (Build specification job).

set -Eeuo pipefail
shopt -s inherit_errexit 2>/dev/null || true

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd -P)"

cd -- "${REPO_ROOT}"

# Collect "<id> <file>" for every `:id:` declaration in spec source, skipping
# the generated _build/ tree.
mapfile -t pairs < <(
  find spec -name '*.rst' -not -path '*/_build/*' -print0 \
    | xargs -0 grep -HoE '^\s*:id:\s+[A-Za-z0-9_]+' \
    | sed -E 's/^([^:]+):[[:space:]]*:id:[[:space:]]+([A-Za-z0-9_]+)/\2 \1/' \
    | sort
)

# Find IDs that appear more than once.
mapfile -t dup_ids < <(printf '%s\n' "${pairs[@]}" | awk '{print $1}' | uniq -d)

if [[ ${#dup_ids[@]} -eq 0 ]]; then
  echo "check-duplicate-need-ids: OK — no duplicate :id: in spec/ source."
  exit 0
fi

echo "check-duplicate-need-ids: FAIL — duplicate need-ID(s) defined in spec/:" >&2
for id in "${dup_ids[@]}"; do
  echo "  ${id}:" >&2
  printf '%s\n' "${pairs[@]}" | awk -v id="${id}" '$1 == id {print "    " $2}' >&2
done
exit 1
