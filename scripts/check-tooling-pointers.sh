#!/usr/bin/env bash
# Guard against rot in the project-local Claude Code agent/skill tooling.
#
# Committed agents (.claude/agents/*.md) and skills (.claude/skills/*/SKILL.md)
# point at canonical repo files ("read these first", "source of truth"). When
# those files move or get renamed, the pointers silently rot. This lint parses
# every such file, extracts the repo-relative paths they reference, and exits
# non-zero listing any that no longer exist on disk.
#
# What counts as a "path reference" (kept deliberately simple and robust):
#   An inline-code span — text wrapped in single backticks `like/this` — that
#     (a) consists solely of path-safe characters [A-Za-z0-9._/+-], AND
#     (b) contains at least one '/', AND
#     (c) is repo-relative (does not start with '/').
#   Such a token is checked for existence relative to the repo root.
#
# This rule, by design:
#   * ignores narrative prose (only backticked tokens are considered);
#   * ignores bare root-level filenames like `CONTEXT.md` or `SKILL.md` (no '/')
#     — these are often referenced narratively or "if present", and the
#     canonical pointers worth guarding always live under a directory;
#   * excludes URLs (contain ':' -> fail the char-class), shell/code fragments
#     (spaces, '=', '{', etc.), globs ('*'), and `<placeholder>` templates
#     ('<','>' -> fail the char-class);
#   * ignores absolute OS paths like `/dev/shm` (leading '/' -> not
#     repo-relative);
#   * skips fenced code blocks (```...```) entirely, so example snippets that
#     mention paths-to-be-created do not trip the guard.
#
# No-ops cleanly (exit 0) when there are zero agent/skill files.
#
# Used by .pre-commit-config.yaml and .github/workflows/ci.yml.

set -Eeuo pipefail
shopt -s inherit_errexit 2>/dev/null || true

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd -P)"

shopt -s nullglob
files=(
    "${REPO_ROOT}"/.claude/agents/*.md
    "${REPO_ROOT}"/.claude/skills/*/SKILL.md
)
shopt -u nullglob

if [[ ${#files[@]} -eq 0 ]]; then
    echo "check-tooling-pointers: ok — no agent/skill files to check"
    exit 0
fi

missing=()
for f in "${files[@]}"; do
    rel_f="${f#"${REPO_ROOT}/"}"
    # Drop fenced code blocks, pull inline-backtick spans, strip the backticks.
    while IFS= read -r token; do
        [[ -n "${token}" ]] || continue
        # (a) path-safe chars only, (b) contains a slash, (c) repo-relative.
        [[ "${token}" =~ ^[A-Za-z0-9._/+-]+$ ]] || continue
        [[ "${token}" == */* ]] || continue
        [[ "${token}" == /* ]] && continue
        if [[ ! -e "${REPO_ROOT}/${token}" ]]; then
            missing+=("  ${rel_f}  ->  ${token}")
        fi
    done < <(
        awk '/^```/ { fence = !fence; next } !fence' "${f}" \
            | grep -oE '`[^`]+`' \
            | sed 's/`//g'
    )
done

if [[ ${#missing[@]} -gt 0 ]]; then
    # De-duplicate while preserving order.
    cat >&2 <<EOF
error: agent/skill tooling references repo paths that do not exist on disk.
A canonical pointer has rotted — fix the path or update the reference:

$(printf '%s\n' "${missing[@]}" | awk '!seen[$0]++')

(A "path reference" is a backticked token containing '/'; see the header of
$(basename "${BASH_SOURCE[0]}") for the exact rule.)
EOF
    exit 1
fi

echo "check-tooling-pointers: ok — all ${#files[@]} agent/skill file(s) reference existing paths"
