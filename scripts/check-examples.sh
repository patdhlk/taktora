#!/usr/bin/env bash
# Build, clippy, and (when marked) run every standalone example under
# the top-level examples/ directory. Used by .github/workflows/ci.yml.
#
# Refuses to proceed unless every example reports `off` from
# scripts/examples-local.sh, so a contributor who commits with the
# toggle on cannot silently ship a CI pass that built against local
# paths.

set -Eeuo pipefail
shopt -s inherit_errexit 2>/dev/null || true

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd -P)"
readonly EXAMPLES_DIR="${REPO_ROOT}/examples"

if [[ ! -d "${EXAMPLES_DIR}" ]]; then
    echo "no examples/ directory; nothing to check"
    exit 0
fi

# 1. Gate on toggle state. Capture once: a naive $(...) would mask the
#    inner script's nonzero exit under set -e, so handle that explicitly.
echo "==> verifying every example is in 'off' state"
gate_output=""
if ! gate_output="$("${SCRIPT_DIR}/examples-local.sh" status)"; then
    echo "${gate_output}"
    echo "FAIL: examples-local.sh status reports inconsistent state." >&2
    echo "      Run 'scripts/examples-local.sh off' before committing." >&2
    exit 1
fi
echo "${gate_output}"
if grep -q $'^on\t' <<<"${gate_output}"; then
    echo "FAIL: at least one example has its [patch.crates-io] block active." >&2
    echo "      Run 'scripts/examples-local.sh off' before committing." >&2
    exit 1
fi

# 2. Guard against silently-dropped [patch.crates-io] overrides. Turns the
#    local-deps patches on, re-resolves each example, and fails if any
#    taktora-* dep still resolves to a registry source (a dropped patch that
#    would build stale published crates). Self-contained: it restores the tree
#    (toggle off + git checkout of manifests/lockfiles) via its own trap.
echo "==> checking example patch skew (local-deps overrides actually apply)"
"${SCRIPT_DIR}/check-example-patch-skew.sh"

# 3. Iterate.
shopt -s nullglob
exit_code=0

# Byte-for-byte restore of one example's manifest + lockfile from a backup dir,
# then drop the backup. Undoes the local-deps patch toggle so the working tree
# ends exactly as it started (mirrors check-example-patch-skew.sh's cleanup).
restore_manifest() {
    local dir="$1" bkdir="$2"
    cp "${bkdir}/Cargo.toml" "${dir}/Cargo.toml"
    if [[ -f "${bkdir}/Cargo.lock" ]]; then
        cp "${bkdir}/Cargo.lock" "${dir}/Cargo.lock"
    else
        rm -f "${dir}/Cargo.lock"
    fi
    rm -rf "${bkdir}"
}

# Build + clippy (+ optional run) one example. Returns nonzero on any failed
# phase. The manifest must already be in the desired patch state.
build_example() {
    local dir="$1" name="$2" rc=0
    pushd "${dir}" > /dev/null
    if ! cargo build --release; then
        echo "FAIL: cargo build for ${name}" >&2
        rc=1
    elif ! cargo clippy --release --all-targets -- -D warnings; then
        echo "FAIL: cargo clippy for ${name}" >&2
        rc=1
    elif [[ -f .runnable ]]; then
        # `timeout` is GNU coreutils (ubuntu-latest CI). Local macOS
        # development needs `brew install coreutils` and `gtimeout`.
        if ! timeout 30s cargo run --release -- --ticks 5; then
            echo "FAIL: cargo run for ${name}" >&2
            rc=1
        fi
    else
        echo "skipping run (no .runnable marker)"
    fi
    popd > /dev/null
    return "${rc}"
}

for manifest in "${EXAMPLES_DIR}"/*/Cargo.toml; do
    dir="$(dirname "${manifest}")"
    name="$(basename "${dir}")"
    echo "::group::${name}"
    start_ts=$(date +%s)

    # Examples carrying a .local-deps marker demonstrate unreleased in-tree API
    # that no published crate exposes yet, so they cannot build against
    # crates.io. Toggle their [patch.crates-io] block on (building against the
    # in-tree crates/) and re-resolve, then restore the committed off-state
    # manifest + lockfile afterwards so the tree is unchanged. The patch-skew
    # guard above already verified the patch actually applies.
    local_deps_backup=""
    if [[ -f "${dir}/.local-deps" ]]; then
        echo "local-deps example: building against in-tree crates/ (patch on)"
        local_deps_backup="$(mktemp -d)"
        cp "${manifest}" "${local_deps_backup}/Cargo.toml"
        [[ -f "${dir}/Cargo.lock" ]] && cp "${dir}/Cargo.lock" "${local_deps_backup}/Cargo.lock"
        EXAMPLES_LOCAL_FIXTURE="${manifest}" "${SCRIPT_DIR}/examples-local.sh" on > /dev/null
        # Force a fresh resolution against the now-active patch: a committed
        # lock pinning the published version would otherwise leave the path
        # override unused (cargo keeps the satisfying locked source).
        ( cd "${dir}" && cargo generate-lockfile > /dev/null 2>&1 ) || true
    fi

    if build_example "${dir}" "${name}"; then
        failed=0
    else
        exit_code=1
        failed=1
    fi

    if [[ -n "${local_deps_backup}" ]]; then
        restore_manifest "${dir}" "${local_deps_backup}"
    fi

    if [[ ${failed} -eq 0 ]]; then
        end_ts=$(date +%s)
        echo "ok: ${name} (took $((end_ts - start_ts))s)"
    fi
    echo "::endgroup::"
done

exit "${exit_code}"
