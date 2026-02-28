#!/usr/bin/env bash
set -euo pipefail

# Tier-1 synthetic performance gate for `specgate check`.
# Defaults are intentionally conservative and can be tuned in CI.
: "${SPECGATE_PERF_MODULES:=120}"
: "${SPECGATE_PERF_FILES_PER_MODULE:=4}"
: "${SPECGATE_PERF_BUDGET_MS:=7000}"

echo "Running perf budget test with:"
echo "  SPECGATE_PERF_MODULES=${SPECGATE_PERF_MODULES}"
echo "  SPECGATE_PERF_FILES_PER_MODULE=${SPECGATE_PERF_FILES_PER_MODULE}"
echo "  SPECGATE_PERF_BUDGET_MS=${SPECGATE_PERF_BUDGET_MS}"

cargo test --locked --test perf_budget -- --nocapture
