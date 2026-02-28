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

validate_yaml_files() {
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
}

scan_forbidden_patterns() {
  local forbidden_pattern='No unreleased code changes are currently queued|@stable|channel = "stable"|your-org/specgate'
  if rg -n -e "$forbidden_pattern" . --glob '!scripts/release/preflight.sh'; then
    echo "Blocked: forbidden pattern found in the repository."
    return 1
  fi
}

run_step "Running CI gate" ./scripts/ci/mvp_gate.sh
run_step "Running cargo tests" cargo test --all-targets --locked
run_step "Running release build" cargo build --release --locked
run_step "Validating required YAML files with Ruby" validate_yaml_files
run_step "Scanning for forbidden patterns" scan_forbidden_patterns

printf '\nPreflight checks passed.\n'
