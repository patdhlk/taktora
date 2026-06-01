#!/usr/bin/env bash
# Guard against the release-plz dev-dependency publish-ordering trap.
#
# release-plz topologically sorts crates for `cargo publish` using only the
# *normal* (and build) dependency graph — it ignores dev-dependency edges
# (upstream bug https://github.com/release-plz/release-plz/issues/2697). But
# `cargo publish`'s verify step *does* resolve dev-dependencies against the
# registry. So a published crate that dev-depends on a sibling workspace crate
# can be ordered *before* that sibling, and when release-plz has just bumped the
# sibling, `cargo publish` rewrites the dev-dep to `^<new-version>` and then
# fails to resolve it from the index — aborting the release half-applied.
#
# A dev-dep edge is only dangerous when there is no *parallel* normal/build edge
# to force the order, and only when the dev-dep carries a version requirement
# (a path-only dev-dep is stripped from the published manifest entirely, so it
# is safe). This lint flags exactly that combination in publishable crates.
#
# Fix a violation by moving the offending test into a `publish = false`
# `*-tests` crate (preferred — see crates/taktora-executor-tests), or by making
# the dev-dep path-only.
#
# Used by .github/workflows/ci.yml.

set -Eeuo pipefail
shopt -s inherit_errexit 2>/dev/null || true

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd -P)"

# For every publishable package (`.publish == null`), flag a version-bearing
# (`.req != "*"`) `taktora-*` dev-dependency that has no parallel normal/build
# edge to the same crate.
violations="$(
    cargo metadata --no-deps --format-version 1 --manifest-path "${REPO_ROOT}/Cargo.toml" \
    | jq -r '
        .packages[]
        | select(.publish == null)                              # publishable to crates.io
        | . as $pkg
        | ( $pkg.dependencies
            | map(select(.kind == null or .kind == "build") | .name) ) as $hard
        | $pkg.dependencies[]
        | select(.kind == "dev")
        | select(.name | startswith("taktora-"))
        | select(.req != "*")                                   # path-only (req "*") is safe
        | select(.name as $n | ($hard | index($n)) | not)       # no normal/build edge protects it
        | "  \($pkg.name)  ->  \(.name)  (dev-dep, req \"\(.req)\", no normal/build edge)"
    '
)"

if [[ -n "${violations}" ]]; then
    cat >&2 <<EOF
error: publishable crate(s) carry a version-bearing internal dev-dependency with
no parallel normal/build edge. release-plz may publish the dependent before the
dependency, aborting the release (see issue #27 / release-plz#2697):

${violations}

Fix: move the offending test into a 'publish = false' *-tests crate (see
crates/taktora-executor-tests), or make the dev-dep path-only (no version).
EOF
    exit 1
fi

echo "check-publish-deps: ok — no unprotected internal dev-dependencies in publishable crates"
