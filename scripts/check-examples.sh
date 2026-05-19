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

# 2. Iterate.
shopt -s nullglob
exit_code=0
for manifest in "${EXAMPLES_DIR}"/*/Cargo.toml; do
    dir="$(dirname "${manifest}")"
    name="$(basename "${dir}")"
    echo "::group::${name}"
    start_ts=$(date +%s)

    pushd "${dir}" > /dev/null
    if ! cargo build --release; then
        echo "FAIL: cargo build for ${name}" >&2
        exit_code=1
        popd > /dev/null
        echo "::endgroup::"
        continue
    fi
    if ! cargo clippy --release --all-targets -- -D warnings; then
        echo "FAIL: cargo clippy for ${name}" >&2
        exit_code=1
        popd > /dev/null
        echo "::endgroup::"
        continue
    fi
    if [[ -f .runnable ]]; then
        # `timeout` is GNU coreutils (ubuntu-latest CI). Local macOS
        # development needs `brew install coreutils` and `gtimeout`.
        if ! timeout 30s cargo run --release -- --ticks 5; then
            echo "FAIL: cargo run for ${name}" >&2
            exit_code=1
            popd > /dev/null
            echo "::endgroup::"
            continue
        fi
    else
        echo "skipping run (no .runnable marker)"
    fi
    popd > /dev/null

    end_ts=$(date +%s)
    echo "ok: ${name} (took $((end_ts - start_ts))s)"
    echo "::endgroup::"
done

exit "${exit_code}"
