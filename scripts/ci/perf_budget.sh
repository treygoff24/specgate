#!/usr/bin/env bash
set -euo pipefail

# Tier-1 synthetic performance gate for `specgate check`.
# Defaults are authoritative here; the CI workflow defers to these values.
# Override via repo-level variables (Settings > Variables) or env vars.
: "${SPECGATE_PERF_MODULES:=120}"
: "${SPECGATE_PERF_FILES_PER_MODULE:=4}"
: "${SPECGATE_PERF_BUDGET_MS:=7000}"

validate_numeric() {
  local var_name="$1"
  local value="$2"
  if ! [[ "$value" =~ ^[0-9]+$ ]]; then
    echo "Error: $var_name must be a positive integer, got: '$value'" >&2
    exit 1
  fi
}

validate_numeric "SPECGATE_PERF_MODULES" "$SPECGATE_PERF_MODULES"
validate_numeric "SPECGATE_PERF_FILES_PER_MODULE" "$SPECGATE_PERF_FILES_PER_MODULE"
validate_numeric "SPECGATE_PERF_BUDGET_MS" "$SPECGATE_PERF_BUDGET_MS"

echo "Running perf budget test with:"
echo "  SPECGATE_PERF_MODULES=${SPECGATE_PERF_MODULES}"
echo "  SPECGATE_PERF_FILES_PER_MODULE=${SPECGATE_PERF_FILES_PER_MODULE}"
echo "  SPECGATE_PERF_BUDGET_MS=${SPECGATE_PERF_BUDGET_MS}"

cargo test --locked --test perf_budget -- --nocapture
