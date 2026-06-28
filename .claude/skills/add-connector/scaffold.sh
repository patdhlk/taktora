#!/usr/bin/env bash
# Scaffold the two-crate skeleton for a new taktora connector.
#
# Creates crates/taktora-connector-<proto>/src and
# crates/taktora-connector-<proto>-tests/tests. The split is mandatory: the
# tests crate is publish=false and holds the internal dev-deps (see SKILL.md
# step 2 and docs/guides/adding-a-connector.md). This only makes the
# directories; fill the manifests/sources by copying crates/taktora-connector-can.
#
# Idempotent: refuses to overwrite an existing connector crate.
#
# Usage: .claude/skills/add-connector/scaffold.sh <proto>
#   e.g. .claude/skills/add-connector/scaffold.sh mqtt

set -Eeuo pipefail
shopt -s inherit_errexit 2>/dev/null || true

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
# .claude/skills/add-connector -> repo root is three levels up.
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/../../.." && pwd -P)"

if [[ $# -ne 1 ]]; then
    echo "usage: $0 <proto>   (e.g. $0 mqtt)" >&2
    exit 2
fi

proto="$1"
if [[ ! "${proto}" =~ ^[a-z][a-z0-9]*$ ]]; then
    echo "error: <proto> must be lowercase alphanumeric, starting with a letter (got '${proto}')" >&2
    exit 2
fi

main_crate="${REPO_ROOT}/crates/taktora-connector-${proto}"
tests_crate="${REPO_ROOT}/crates/taktora-connector-${proto}-tests"

for d in "${main_crate}" "${tests_crate}"; do
    if [[ -e "${d}" ]]; then
        echo "error: ${d} already exists — refusing to clobber" >&2
        exit 1
    fi
done

mkdir -p "${main_crate}/src"
mkdir -p "${tests_crate}/tests"

cat <<EOF
scaffolded:
  ${main_crate#"${REPO_ROOT}/"}/src
  ${tests_crate#"${REPO_ROOT}/"}/tests

next steps (see docs/guides/adding-a-connector.md):
  1. Copy crates/taktora-connector-can/* into the new crate and rename.
  2. Make the -tests crate 'publish = false' and move internal dev-deps there.
  3. Add both crates to root Cargo.toml [workspace.members]; add the published
     crate to [workspace.dependencies].
  4. Run ./scripts/check-publish-deps.sh to verify the publish order.
EOF
