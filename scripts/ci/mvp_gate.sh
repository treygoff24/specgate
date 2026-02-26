#!/usr/bin/env bash
set -uo pipefail

SUMMARY_FILE="${GITHUB_STEP_SUMMARY:-}"

overall_status=0
runtime_failures=()
policy_failures=()
contract_failures=()

run_step() {
  local category="$1"
  local label="$2"
  shift 2

  echo "::group::${label}"
  echo "$ $*"

  "$@"
  local exit_code=$?

  if [[ ${exit_code} -eq 0 ]]; then
    echo "✅ ${label}"
  else
    overall_status=1
    echo "❌ ${label} (exit ${exit_code})"
    echo "::error title=${category} failure::${label} failed with exit ${exit_code}"

    case "${category}" in
      runtime)
        runtime_failures+=("${label} (exit ${exit_code})")
        ;;
      policy)
        policy_failures+=("${label} (exit ${exit_code})")
        ;;
      contract)
        contract_failures+=("${label} (exit ${exit_code})")
        ;;
    esac
  fi

  echo "::endgroup::"
}

append_failure_group() {
  local title="$1"
  shift
  local -a items=("$@")

  if [[ ${#items[@]} -eq 0 ]]; then
    return
  fi

  echo "- ${title}:"
  for item in "${items[@]}"; do
    echo "  - ${item}"
  done
}

render_summary() {
  echo "## Specgate MVP Merge Gate"
  echo
  echo "### Required command sequence"
  echo "1. cargo fmt --check"
  echo "2. cargo clippy --all-targets -- -D warnings"
  echo "3. cargo test --test contract_fixtures"
  echo "4. cargo test --test golden_corpus"
  echo "5. cargo test --test tier_a_golden"
  echo "6. cargo test --test mvp_gate_baseline"
  echo

  if [[ ${overall_status} -eq 0 ]]; then
    echo "### Result: ✅ PASS"
    echo
    echo "Pass criteria met:"
    echo "- No runtime/setup failures"
    echo "- No contract drift in contract fixtures, golden corpus, or Tier A deterministic gate"
    echo "- Baseline behavior checks passed (baseline hits report-only; new violations fail policy gate)"
  else
    echo "### Result: ❌ FAIL"
    echo
    echo "Failure categories:"
    append_failure_group "Runtime/setup failure" "${runtime_failures[@]}"
    append_failure_group "Contract drift" "${contract_failures[@]}"
    append_failure_group "Policy failure" "${policy_failures[@]}"
  fi
}

emit_summary() {
  render_summary

  if [[ -n "${SUMMARY_FILE}" ]]; then
    render_summary >>"${SUMMARY_FILE}"
  fi
}

run_step runtime "Formatting" cargo fmt --check
run_step runtime "Linting" cargo clippy --all-targets -- -D warnings

run_step contract "Contract fixtures" cargo test --test contract_fixtures
run_step contract "Golden corpus" cargo test --test golden_corpus
run_step contract "Tier A deterministic gate" cargo test --test tier_a_golden

run_step policy "Baseline/new-violation behavior" cargo test --test mvp_gate_baseline

emit_summary

exit ${overall_status}
