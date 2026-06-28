#!/usr/bin/env bash
# Complexity gate — second guarding tool alongside clippy.
# Hard-fails when a function in crates/*/src exceeds the cyclomatic / exit-point
# bar. Maintainability Index is advisory only (degenerate on tiny files).
# Spec: docs/superpowers/specs/2026-06-02-rust-code-analysis-complexity-gate-design.md
set -euo pipefail
# Force the C locale: `printf '%.0f'` is locale-sensitive and would misparse
# rca's dot-decimal floats (e.g. "25.0") under a comma-decimal locale
# (de_DE/fr_FR/...), silently missing violations or aborting mid-scan.
export LC_ALL=C

CC_MAX=20
NEXITS_MAX=5
MI_ADVISORY_COUNT=10

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# Default scan root = the workspace crates; override with $1 for the self-test.
SCAN_ROOT="${1:-$ROOT/crates}"

# Graceful skip when tooling is absent (keeps the pre-push hook friction-free;
# CI always installs the tool, so CI remains a hard gate).
if ! command -v rust-code-analysis-cli >/dev/null 2>&1 || ! command -v jq >/dev/null 2>&1; then
  echo "complexity gate: rust-code-analysis-cli and/or jq not found — skipping."
  echo "  install: cargo install --locked rust-code-analysis-cli   (and your platform's jq)"
  exit 0
fi

# Production scope: crates/*/src/**.rs, excluding build.rs. For the self-test the
# scan root is a fixtures dir with no */src/* layout, so fall back to all *.rs.
# The `/src/` match is ANCHORED to SCAN_ROOT (`$SCAN_ROOT/*/src/*`), not a bare
# `*/src/*`: an unanchored pattern matches the absolute checkout prefix too, so a
# clone under a path that itself contains `/src/` (e.g. ~/src/...) would wrongly
# pull in non-src files like tests/golden snapshots. Anchoring keeps the scope at
# the documented `crates/<crate>/src/**` regardless of where the repo lives.
# Note: mapfile requires bash 4+; use read-loop for portability with macOS bash 3.
files=()
while IFS= read -r line; do files+=("$line"); done < <(find "$SCAN_ROOT" -path "$SCAN_ROOT/*/src/*" -name '*.rs' ! -name 'build.rs' | sort)
if [ "${#files[@]}" -eq 0 ]; then
  while IFS= read -r line; do files+=("$line"); done < <(find "$SCAN_ROOT" -name '*.rs' | sort)
fi

# No Rust sources under the scan root → nothing to gate. Exit clean. (Also
# avoids a `set -u` "unbound variable" on the empty-array loop under bash 3.2.)
if [ "${#files[@]}" -eq 0 ]; then
  echo "complexity gate: OK — no Rust source files under $SCAN_ROOT."
  exit 0
fi

violations=0
mi_report=""

for f in "${files[@]}"; do
  json="$(rust-code-analysis-cli -m -p "$f" -O json 2>/dev/null)" || continue
  [ -z "$json" ] && continue

  # Per-function cyclomatic / nexits breaches. Note: rca's `.sum` rolls nested
  # closures into the enclosing function, so a function may be flagged for
  # complexity its own body does not show — extracting the closure fixes both.
  while IFS=$'\t' read -r cc ne name; do
    [ -z "${name:-}" ] && continue
    cci=$(printf '%.0f' "$cc"); nei=$(printf '%.0f' "$ne")
    if [ "$cci" -gt "$CC_MAX" ]; then
      echo "FAIL cyclomatic=$cci (> $CC_MAX)  $f :: $name"
      violations=$((violations + 1))
    fi
    if [ "$nei" -gt "$NEXITS_MAX" ]; then
      echo "FAIL nexits=$nei (> $NEXITS_MAX)  $f :: $name"
      violations=$((violations + 1))
    fi
  done < <(printf '%s' "$json" | jq -r '
    [.. | objects | select(.kind == "function")][]
    | "\(.metrics.cyclomatic.sum)\t\(.metrics.nexits.sum)\t\(.name)"')

  # MI advisory — only for files that actually contain functions (skip stubs).
  read -r nfns mi < <(printf '%s' "$json" | jq -r '
    "\([.. | objects | select(.kind == "function")] | length) \(.metrics.mi.mi_visual_studio // 999)"') || true
  if [ "${nfns:-0}" -gt 0 ]; then
    mi_report+="$mi"$'\t'"$f"$'\n'
  fi
done

echo
echo "Maintainability Index advisory (lowest $MI_ADVISORY_COUNT; VS scale 0-100, higher = better):"
# `|| true`: `head` closing the pipe early makes `sort` exit on SIGPIPE (141),
# which under `pipefail` would fail the whole gate. This is the *advisory* print
# only — the real gate is the `violations`/`exit 1` block below — so a broken
# pipe here must never fail the script.
printf '%s' "$mi_report" | sort -t$'\t' -k1 -n | head -n "$MI_ADVISORY_COUNT" \
  | awk -F'\t' '{printf "  mi=%6.1f  %s\n", $1, $2}' || true
echo

if [ "$violations" -gt 0 ]; then
  echo "complexity gate: FAILED — $violations violation(s). Limits: cyclomatic <= $CC_MAX, nexits <= $NEXITS_MAX per function."
  exit 1
fi
echo "complexity gate: OK — all functions within cyclomatic <= $CC_MAX, nexits <= $NEXITS_MAX."
