#!/usr/bin/env bash
# Toggle every examples/<name>/Cargo.toml's [patch.crates-io] block on/off.
#
# Usage:
#   scripts/examples-local.sh on        # uncomment all patch lines
#   scripts/examples-local.sh off       # re-comment all patch lines
#   scripts/examples-local.sh status    # report on/off; exit 1 if inconsistent

set -Eeuo pipefail
shopt -s inherit_errexit 2>/dev/null || true

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd -P)"

readonly MARKER_OPEN='# >>> taktora-examples-local-deps >>>'
readonly MARKER_CLOSE='# <<< taktora-examples-local-deps <<<'

cmd="${1:-status}"
case "${cmd}" in
    on|off|status) ;;
    *)
        echo "usage: $0 {on|off|status}" >&2
        exit 2
        ;;
esac

# Allow tests to point at a single fixture instead of the examples dir.
declare -a MANIFESTS=()
if [[ -n "${EXAMPLES_LOCAL_FIXTURE:-}" ]]; then
    MANIFESTS=("${EXAMPLES_LOCAL_FIXTURE}")
else
    while IFS= read -r -d '' f; do
        MANIFESTS+=("$f")
    done < <(find "${REPO_ROOT}/examples" -mindepth 2 -maxdepth 2 -name Cargo.toml -print0 2>/dev/null)
fi

if [[ ${#MANIFESTS[@]} -eq 0 ]]; then
    echo "no example manifests found under ${REPO_ROOT}/examples" >&2
    exit 0
fi

manifest_state() {
    # Print "on" / "off" / "missing" for a single manifest based on the first
    # line inside the marker block. BSD awk reserves `close` as a built-in,
    # so the marker variables are named mopen/mclose.
    local file="$1"
    awk -v mopen="${MARKER_OPEN}" -v mclose="${MARKER_CLOSE}" '
        BEGIN { inside = 0; first_inside = ""; }
        # Marker match is whole-line (no leading-whitespace tolerance) on purpose:
        # manifests must keep the markers at column 0 to avoid silently corrupting
        # nested or indented comment blocks.
        $0 == mopen { inside = 1; next }
        $0 == mclose { inside = 0; next }
        inside && first_inside == "" { first_inside = $0 }
        END {
            if (first_inside == "") { print "missing"; exit 0 }
            if (first_inside ~ /^# /) { print "off"; exit 0 }
            print "on"
        }
    ' "$file"
}

apply_state() {
    local file="$1" target="$2"
    local tmp
    tmp="$(mktemp)"
    awk -v mopen="${MARKER_OPEN}" -v mclose="${MARKER_CLOSE}" -v target="${target}" '
        BEGIN { inside = 0 }
        # Marker match is whole-line (no leading-whitespace tolerance) on purpose:
        # manifests must keep the markers at column 0 to avoid silently corrupting
        # nested or indented comment blocks.
        $0 == mopen { inside = 1; print; next }
        $0 == mclose { inside = 0; print; next }
        inside {
            if (target == "on") {
                # Strip a single leading "# " if present.
                sub(/^# /, "")
            } else {
                # Add "# " unless the line is already prefixed.
                if ($0 !~ /^# /) {
                    $0 = "# " $0
                }
            }
        }
        { print }
    ' "$file" > "$tmp"
    mv "$tmp" "$file"
}

case "${cmd}" in
    on|off)
        for f in "${MANIFESTS[@]}"; do
            s="$(manifest_state "$f")"
            if [[ "$s" == "missing" ]]; then
                echo "FAIL: marker block missing in $f" >&2
                exit 2
            fi
            if [[ "$s" == "$cmd" ]]; then
                echo "WARN: $f already $cmd; skipping" >&2
                continue
            fi
            apply_state "$f" "$cmd"
        done
        if [[ "$cmd" == "on" ]]; then
            cat >&2 <<EOF
WARNING: do not commit examples/ while patches are active.
         Run scripts/examples-local.sh off before committing.
EOF
        fi
        ;;
    status)
        any_on=0
        any_off=0
        for f in "${MANIFESTS[@]}"; do
            s="$(manifest_state "$f")"
            printf '%s\t%s\n' "$s" "$f"
            [[ "$s" == "on" ]] && any_on=1
            [[ "$s" == "off" ]] && any_off=1
        done
        if [[ $any_on -eq 1 && $any_off -eq 1 ]]; then
            echo "FAIL: inconsistent state across examples" >&2
            exit 1
        fi
        ;;
esac
