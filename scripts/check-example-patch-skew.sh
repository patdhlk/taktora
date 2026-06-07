#!/usr/bin/env bash
# Guard against silently-dropped [patch.crates-io] overrides in examples/.
#
# Each examples/<name> crate opts out of the workspace and pins PUBLISHED
# taktora-* versions. scripts/examples-local.sh on uncomments a
# [patch.crates-io] block redirecting those deps at the in-tree crates/.
# A cargo patch only applies when the local crate's version satisfies the
# example's version requirement; once the workspace bumps a crate past an
# example's pin (local 0.2.x vs pinned "0.1"), cargo SILENTLY drops the
# patch and resolves the stale published crate from the registry. CI stays
# green while local-dev debugging tests the wrong code. This bit twice on
# real hardware.
#
# This check turns the patches on, resolves each example's dependency graph,
# and FAILS if any taktora-* package still resolves to a registry source.
# Path-resolved (patched) crates have a null `source` in cargo metadata.
#
# It is self-contained and runnable standalone; check-examples.sh invokes it
# as a phase before its build loop. Every example manifest + lockfile is
# backed up byte-for-byte before the toggle and restored on exit (even on
# failure), so uncommitted local edits survive and the tree ends exactly as
# it started.

set -Eeuo pipefail
shopt -s inherit_errexit 2>/dev/null || true

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd -P)"
readonly EXAMPLES_DIR="${REPO_ROOT}/examples"
readonly CRATES_DIR="${REPO_ROOT}/crates"

readonly REGISTRY_PREFIX='registry+'

# Byte-for-byte backups of every example manifest + lockfile, taken before
# the patch toggle. Restoring these (rather than `git checkout`) preserves
# any uncommitted edits the developer has in flight, so the guard is safe to
# run on a dirty tree.
BACKUP_DIR=""

backup_manifests() {
    BACKUP_DIR="$(mktemp -d)"
    local manifest dir name
    for manifest in "${EXAMPLES_DIR}"/*/Cargo.toml; do
        dir="$(dirname "${manifest}")"
        name="$(basename "${dir}")"
        cp "${manifest}" "${BACKUP_DIR}/${name}.Cargo.toml"
        if [[ -f "${dir}/Cargo.lock" ]]; then
            cp "${dir}/Cargo.lock" "${BACKUP_DIR}/${name}.Cargo.lock"
        fi
    done
}

# Restore the working tree exactly as it was before the toggle + resolution,
# then drop the backups. Runs on every exit path via trap.
cleanup() {
    [[ -n "${BACKUP_DIR}" && -d "${BACKUP_DIR}" ]] || return 0
    local manifest dir name
    for manifest in "${EXAMPLES_DIR}"/*/Cargo.toml; do
        dir="$(dirname "${manifest}")"
        name="$(basename "${dir}")"
        if [[ -f "${BACKUP_DIR}/${name}.Cargo.toml" ]]; then
            cp "${BACKUP_DIR}/${name}.Cargo.toml" "${manifest}"
        fi
        if [[ -f "${BACKUP_DIR}/${name}.Cargo.lock" ]]; then
            cp "${BACKUP_DIR}/${name}.Cargo.lock" "${dir}/Cargo.lock"
        elif [[ -f "${dir}/Cargo.lock" ]]; then
            # The lockfile did not exist before; resolution created it.
            rm -f "${dir}/Cargo.lock"
        fi
    done
    rm -rf "${BACKUP_DIR}"
}

# Print the in-tree version of a single crate, or "(no in-tree crate)" when it
# has no crates/<crate>/Cargo.toml. Kept free of associative arrays so the
# script runs under the macOS-stock bash 3.2 used on dev machines.
local_crate_version() {
    local crate="$1" manifest="${CRATES_DIR}/$1/Cargo.toml" version=""
    if [[ -f "${manifest}" ]]; then
        version="$(awk -F'"' '/^version[[:space:]]*=/ { print $2; exit }' "${manifest}")"
    fi
    printf '%s' "${version:-(no in-tree crate)}"
}

# Reads cargo-metadata JSON on stdin; emits one "<crate>|<req>|<reg_version>"
# line per taktora-* package whose resolved source is a registry (the patch
# was dropped). Path-resolved (patched) packages have a null source and are
# skipped. The req is the root crate's declared version requirement, or a
# transitive marker when the taktora-* crate is not a direct dependency.
read -r -d '' PY_PARSE <<'PY' || true
import json
import sys

registry_prefix = sys.argv[1]
data = json.load(sys.stdin)

root_id = data.get("resolve", {}).get("root")
reqs = {}
for pkg in data["packages"]:
    if pkg["id"] != root_id:
        continue
    for dep in pkg["dependencies"]:
        if dep["name"].startswith("taktora-"):
            reqs[dep["name"]] = dep.get("req", "")

for pkg in data["packages"]:
    name = pkg["name"]
    if not name.startswith("taktora-"):
        continue
    source = pkg.get("source")
    if source and source.startswith(registry_prefix):
        req = reqs.get(name, "(transitive — no direct req)")
        print("{}|{}|{}".format(name, req, pkg["version"]))
PY

# Inspect one example. Emits one "<crate>|<req>|<registry_version>" line per
# taktora-* package that resolved to a registry source. Empty output = clean.
#
# A fresh lockfile is generated first: the committed Cargo.lock often pins a
# registry version that already satisfies the req, so cargo metadata alone
# would keep that locked registry source and never apply the path patch even
# when it is valid. cargo generate-lockfile forces a re-resolution against the
# now-active [patch.crates-io] block, after which a surviving registry source
# is a genuine dropped patch (a real version-req mismatch).
resolve_skew() {
    local dir="$1"
    (
        cd "${dir}"
        cargo generate-lockfile > /dev/null 2>&1
        cargo metadata --format-version 1 2> /dev/null
    ) | python3 -c "${PY_PARSE}" "${REGISTRY_PREFIX}"
}

main() {
    if [[ ! -d "${EXAMPLES_DIR}" ]]; then
        echo "no examples/ directory; nothing to check"
        return 0
    fi

    echo "::group::example patch-skew guard"
    echo "==> turning local-deps patches on"
    backup_manifests
    trap cleanup EXIT
    "${SCRIPT_DIR}/examples-local.sh" on > /dev/null

    shopt -s nullglob
    local exit_code=0 manifest dir name skew crate req reg_version local_version
    for manifest in "${EXAMPLES_DIR}"/*/Cargo.toml; do
        dir="$(dirname "${manifest}")"
        name="$(basename "${dir}")"
        echo "==> resolving ${name}"

        skew="$(resolve_skew "${dir}")"
        [[ -z "${skew}" ]] && continue

        exit_code=1
        while IFS='|' read -r crate req reg_version; do
            [[ -z "${crate}" ]] && continue
            local_version="$(local_crate_version "${crate}")"
            {
                echo "FAIL: ${name}: ${crate} resolved to the registry despite local-deps patch."
                echo "      example requires:  ${req}"
                echo "      registry version:  ${reg_version}"
                echo "      local in-tree:     ${local_version}"
                if [[ "${req}" == \(* ]]; then
                    echo "      Transitive: a direct taktora-* dep above pulled the registry copy;"
                    echo "      fix the offending direct pin and ${crate} will follow the patch."
                else
                    echo "      The patch was silently dropped: local ${local_version} does not satisfy '${req}'."
                    echo "      Bump the ${crate} pin in ${name}/Cargo.toml to the current published major."
                fi
            } >&2
        done <<< "${skew}"
    done

    echo "::endgroup::"
    if [[ ${exit_code} -ne 0 ]]; then
        echo "FAIL: example patch-skew guard found stale registry resolutions (see above)." >&2
    else
        echo "ok: every example resolves all taktora-* deps to in-tree paths"
    fi
    return "${exit_code}"
}

main "$@"
