#!/usr/bin/env bash
# Unsafe gate — enforces `#![forbid(unsafe_code)]` on designated crates and
# reports unsafe usage across the workspace.
# Spec: spec/requirements/tooling/unsafe.rst (FEAT_0201)
set -euo pipefail

# Crates required to forbid unsafe code. Each must have `#![forbid(unsafe_code)]`
# in its lib.rs and zero actual unsafe usage — verified by cargo-geiger.
FORBID_CRATES=(
  taktora-executor
)

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Graceful skip when tooling is absent (keeps the pre-push hook friction-free;
# CI always installs the tool, so CI remains a hard gate).
if ! command -v cargo-geiger >/dev/null 2>&1 || ! command -v jq >/dev/null 2>&1; then
  echo "unsafe gate: cargo-geiger and/or jq not found — skipping."
  echo "  install: cargo install --locked cargo-geiger   (and your platform's jq)"
  exit 0
fi

# Default: run the gate only, skip the slow workspace-wide report.
# CI overrides with GEIGER_REPORT=1 to publish the full artifact.
GEIGER_REPORT="${GEIGER_REPORT:-0}"

# Ensure log directory exists.
mkdir -p "$ROOT/target/geiger"

violations=0

# Gate: hard-fail if any FORBID_CRATES crate uses unsafe or does not forbid it.
for crate in "${FORBID_CRATES[@]}"; do
  manifest="$ROOT/crates/$crate/Cargo.toml"
  if [ ! -f "$manifest" ]; then
    echo "FAIL: FORBID_CRATES lists $crate but $manifest not found"
    violations=$((violations + 1))
    continue
  fi

  # cargo-geiger refuses virtual workspace manifests; needs the absolute path
  # to the member manifest. --all-features ensures cfg-gated unsafe is visible.
  # stderr carries noisy 'Failed to parse file' + cargo JSON; redirect to log.
  logfile="$ROOT/target/geiger/${crate}-gate.log"
  json=$(cargo geiger --manifest-path "$manifest" --all-features --output-format Json 2>"$logfile") || {
    echo "FAIL: cargo-geiger failed for $crate (see $logfile)"
    violations=$((violations + 1))
    continue
  }

  # Find the first-party package in the dependency tree: name matches and the
  # source is a local path (geiger encodes it as {"Path": "file://..."}), not
  # registry/git. cargo-geiger outputs the whole dep tree; filter to our crate.
  pkg_data=$(printf '%s' "$json" | jq -r --arg name "$crate" '
    .packages[]
    | select(.package.id.name == $name and ((.package.id.source | objects | has("Path")) // false))
    | .unsafety
    | @json' | head -n 1)

  if [ -z "$pkg_data" ]; then
    echo "FAIL: $crate not found in geiger output (see $logfile)"
    violations=$((violations + 1))
    continue
  fi

  forbids=$(printf '%s' "$pkg_data" | jq -r '.forbids_unsafe')
  used_fns=$(printf '%s' "$pkg_data" | jq -r '.used.functions.unsafe_')
  used_exprs=$(printf '%s' "$pkg_data" | jq -r '.used.exprs.unsafe_')
  used_impls=$(printf '%s' "$pkg_data" | jq -r '.used.item_impls.unsafe_')
  used_traits=$(printf '%s' "$pkg_data" | jq -r '.used.item_traits.unsafe_')
  used_methods=$(printf '%s' "$pkg_data" | jq -r '.used.methods.unsafe_')

  total_used=$((used_fns + used_exprs + used_impls + used_traits + used_methods))

  if [ "$forbids" != "true" ]; then
    echo "FAIL: $crate does not forbid unsafe (forbids_unsafe=$forbids)"
    violations=$((violations + 1))
  fi

  if [ "$total_used" -gt 0 ]; then
    echo "FAIL: $crate uses unsafe (fns=$used_fns exprs=$used_exprs impls=$used_impls traits=$used_traits methods=$used_methods)"
    violations=$((violations + 1))
  fi
done

# Report (optional): workspace-wide unsafe inventory.
if [ "$GEIGER_REPORT" = "1" ]; then
  # Enumerate workspace members (excluding xtask/*).
  metadata=$(cargo metadata --no-deps --format-version 1 2>/dev/null)
  members=$(printf '%s' "$metadata" | jq -r '.packages[] | select(.manifest_path | contains("/xtask/") | not) | .name')

  report_md="$ROOT/target/geiger/report.md"
  {
    echo "# Unsafe Usage Report"
    echo ""
    echo "| Crate | Forbids Unsafe | Used Unsafe (fns/exprs/impls/traits/methods) | Total |"
    echo "|-------|----------------|-----------------------------------------------|-------|"
  } > "$report_md"

  for member in $members; do
    # Find manifest path for this member.
    member_manifest=$(printf '%s' "$metadata" | jq -r --arg name "$member" '.packages[] | select(.name == $name) | .manifest_path')

    logfile="$ROOT/target/geiger/${member}-report.log"
    json=$(cargo geiger --manifest-path "$member_manifest" --all-features --output-format Json 2>"$logfile") || {
      echo "  (skipped $member: geiger failed)"
      continue
    }

    # Save raw JSON.
    printf '%s' "$json" > "$ROOT/target/geiger/${member}.json"

    # Extract the first-party package.
    pkg_data=$(printf '%s' "$json" | jq -r --arg name "$member" '
      .packages[]
      | select(.package.id.name == $name and ((.package.id.source | objects | has("Path")) // false))
      | .unsafety
      | @json' | head -n 1)

    if [ -z "$pkg_data" ]; then
      continue
    fi

    forbids=$(printf '%s' "$pkg_data" | jq -r '.forbids_unsafe')
    used_fns=$(printf '%s' "$pkg_data" | jq -r '.used.functions.unsafe_')
    used_exprs=$(printf '%s' "$pkg_data" | jq -r '.used.exprs.unsafe_')
    used_impls=$(printf '%s' "$pkg_data" | jq -r '.used.item_impls.unsafe_')
    used_traits=$(printf '%s' "$pkg_data" | jq -r '.used.item_traits.unsafe_')
    used_methods=$(printf '%s' "$pkg_data" | jq -r '.used.methods.unsafe_')
    total_used=$((used_fns + used_exprs + used_impls + used_traits + used_methods))

    # Mark FORBID_CRATES rows with a checkmark.
    marker=""
    for fc in "${FORBID_CRATES[@]}"; do
      if [ "$member" = "$fc" ]; then
        marker=" ✓"
        break
      fi
    done

    echo "| ${member}${marker} | ${forbids} | ${used_fns}/${used_exprs}/${used_impls}/${used_traits}/${used_methods} | ${total_used} |" >> "$report_md"
  done

  echo "" >> "$report_md"
  echo "✓ marks crates in the FORBID gate." >> "$report_md"

  # Append to GitHub Actions step summary if running in CI.
  if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    {
      echo "## Unsafe Gate"
      echo ""
      if [ "$violations" -eq 0 ]; then
        echo "✅ All FORBID_CRATES crates forbid unsafe and have zero unsafe usage."
      else
        echo "❌ $violations violation(s) detected."
      fi
      echo ""
      cat "$report_md"
    } >> "$GITHUB_STEP_SUMMARY"
  fi
fi

echo
if [ "$violations" -gt 0 ]; then
  echo "unsafe gate: FAILED — $violations violation(s). FORBID_CRATES: ${FORBID_CRATES[*]}"
  exit 1
fi
echo "unsafe gate: OK — all FORBID_CRATES forbid unsafe and have zero unsafe usage."
