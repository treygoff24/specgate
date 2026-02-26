#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

step=0
run_step() {
  step=$((step + 1))
  local description=$1
  shift
  printf '\n[%d] %s\n' "$step" "$description"
  "$@"
}

run_step "Running CI gate" ./scripts/ci/mvp_gate.sh
run_step "Running cargo tests" cargo test --all-targets --locked
run_step "Running release build" cargo build --release --locked

run_step "Validating required YAML files with Ruby"
ruby -e '
require "yaml"
files = [
  ".github/workflows/mvp-merge-gate.yml",
  ".github/workflows/release-binaries.yml",
  ".github/workflows/release-asset-verify.yml",
  "docs/examples/specgate-consumer-github-actions.yml",
]
files.each do |path|
  YAML.load_file(path)
  puts "  ✓ #{path}"
end
'

run_step "Scanning for forbidden patterns"
forbidden_pattern='No unreleased code changes are currently queued|@stable|channel = "stable"|your-org/specgate'
if rg -n -E "$forbidden_pattern" .; then
  echo "Blocked: forbidden pattern found in the repository."
  exit 1
fi

printf '\nPreflight checks passed.\n'
