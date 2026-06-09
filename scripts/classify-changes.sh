#!/usr/bin/env bash
# Classify a PR/push diff for CI gating. Writes two GitHub step outputs:
#   code — true unless EVERY changed file is README.md or under spec/
#   spec — true when any changed file is under spec/
#
# This lets doc-only (README.md) and spec-only changes skip the Rust pipeline
# while spec changes still run the spec build. It errs toward running the full
# pipeline whenever the diff base is unavailable (first push, force-push, new
# branch) — never skip real work on uncertainty.
#
# Consumed by .github/workflows/{ci,ci-zenoh,ci-can}.yml via a leading
# `changes` job. Expects EVENT / PR_BASE / PR_HEAD / PUSH_BEFORE / PUSH_SHA in
# the environment (set from the workflow context) and a checkout with full
# history (fetch-depth: 0) so the three-dot merge-base diff resolves.
set -euo pipefail

if [[ "${EVENT:-}" == "pull_request" ]]; then
    base="${PR_BASE:-}"
    head="${PR_HEAD:-}"
else
    base="${PUSH_BEFORE:-}"
    head="${PUSH_SHA:-}"
fi

emit() {
    echo "=> code=$1 spec=$2"
    {
        echo "code=$1"
        echo "spec=$2"
    } >>"${GITHUB_OUTPUT:?GITHUB_OUTPUT not set}"
}

# No usable base (first push to a branch, force-push, or all-zeros sha): don't
# risk skipping real work — run the full pipeline.
zero="0000000000000000000000000000000000000000"
if [[ -z "$base" || "$base" == "$zero" ]] || ! git cat-file -e "${base}^{commit}" 2>/dev/null; then
    echo "diff base unavailable (base='${base}') — running the full pipeline"
    emit true true
    exit 0
fi

files="$(git diff --name-only "${base}...${head}")"
echo "changed files:"
printf '%s\n' "$files" | sed 's/^/  /'

# Empty diff (e.g. a no-op merge commit): be conservative and run everything.
if [[ -z "$files" ]]; then
    emit true true
    exit 0
fi

# Iterate via a here-string so the assignments land in this shell (a pipe into
# `while` would run the loop in a subshell and lose them). Portable to bash 3.2.
code=false
spec=false
while IFS= read -r f; do
    [[ -z "$f" ]] && continue
    if [[ "$f" == spec/* ]]; then
        spec=true
    fi
    if [[ "$f" != "README.md" && "$f" != spec/* ]]; then
        code=true
    fi
done <<<"$files"

emit "$code" "$spec"
