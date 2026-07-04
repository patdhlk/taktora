#!/usr/bin/env bash
#
# generate-sbom.sh — produce per-crate SBOMs (CycloneDX + SPDX) and, in CI,
# attach them to the matching GitHub Release created by release-plz.
#
# Each released crate gets two files, each scoped to *that crate's own*
# transitive dependency graph:
#
#     <pkg>-<version>.cdx.json     CycloneDX 1.6
#     <pkg>-<version>.spdx.json    SPDX 2.3
#
# Both are produced by `cargo-sbom`: one tool, both formats, and unambiguous
# per-crate scoping via `--cargo-package` (which cargo-cyclonedx lacks — it
# writes a `bom.json` next to every member's Cargo.toml with no clean
# single-crate mode).
#
# Two modes:
#
#   CI mode (release-plz.yml) — set RELEASES to release-plz's `releases` output:
#       RELEASES='[{"package_name":"taktora-executor","version":"0.2.0",
#                   "tag":"taktora-executor-v0.2.0"}, ...]' \
#         GH_TOKEN=... ./scripts/generate-sbom.sh
#     Generates SBOMs for every released crate and uploads them to its GitHub
#     Release via `gh release upload`.
#
#   Local mode (manual test) — name one crate on the command line:
#       ./scripts/generate-sbom.sh taktora-executor
#     Writes SBOMs to ./sbom-dist/ and skips the upload.

set -euo pipefail

OUTDIR="${OUTDIR:-sbom-dist}"
mkdir -p "$OUTDIR"

need() {
  command -v "$1" >/dev/null 2>&1 || { echo "::error::missing required tool: $1" >&2; exit 1; }
}
need cargo-sbom
need jq

# crate_version <package> — resolve a crate's version from workspace metadata
# (used in local mode when the caller omits it).
crate_version() {
  cargo metadata --no-deps --format-version 1 \
    | jq -r --arg p "$1" '.packages[] | select(.name == $p) | .version'
}

# gen_one <package> <version> — write both SBOMs for the crate into $OUTDIR.
gen_one() {
  local pkg="$1" version="$2"
  local base="$OUTDIR/${pkg}-${version}"
  echo "→ generating SBOMs for $pkg v$version"
  cargo sbom --cargo-package "$pkg" --output-format spdx_json_2_3       > "${base}.spdx.json"
  cargo sbom --cargo-package "$pkg" --output-format cyclone_dx_json_1_6 > "${base}.cdx.json"
}

# upload <tag> <files...> — attach assets to an existing GitHub Release.
upload() {
  local tag="$1"; shift
  if ! gh release view "$tag" >/dev/null 2>&1; then
    echo "::warning::no GitHub Release for tag '$tag' — skipping SBOM upload"
    return 0
  fi
  gh release upload "$tag" "$@" --clobber
}

if [[ -n "${RELEASES:-}" ]]; then
  # CI mode — iterate over release-plz's `releases` output.
  count=$(jq 'length' <<<"$RELEASES")
  echo "Generating SBOMs for ${count} released crate(s)."
  jq -c '.[]' <<<"$RELEASES" | while read -r rel; do
    pkg=$(jq -r '.package_name' <<<"$rel")
    version=$(jq -r '.version' <<<"$rel")
    tag=$(jq -r '.tag' <<<"$rel")
    gen_one "$pkg" "$version"
    upload "$tag" "$OUTDIR/${pkg}-${version}.spdx.json" "$OUTDIR/${pkg}-${version}.cdx.json"
  done
else
  # Local mode — one crate named on the command line, no upload.
  pkg="${1:?usage: generate-sbom.sh <package> [version]   (or set RELEASES for CI)}"
  version="${2:-$(crate_version "$pkg")}"
  [[ -n "$version" ]] || { echo "::error::could not resolve version for '$pkg'" >&2; exit 1; }
  gen_one "$pkg" "$version"
  echo "Wrote SBOMs to $OUTDIR/"
fi
