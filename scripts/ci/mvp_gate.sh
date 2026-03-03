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
  echo "2. cargo clippy --locked --all-targets -- -D warnings"
  echo "3. cargo test --locked --lib"
  echo "4. cargo test --locked --test contract_fixtures"
  echo "5. cargo test --locked --test contract_validation_fixtures"
  echo "6. cargo test --locked --test contracts_rules_contract_refs"
  echo "7. cargo test --locked --test structured_diagnostics_contracts"
  echo "8. cargo test --locked --test contract_e2e"
  echo "9. cargo test --locked --test contract_e2e_edge"
  echo "10. cargo test --locked --test golden_corpus_gate"
  echo "11. cargo test --locked --test tier_a_golden"
  echo "12. cargo test --locked --test integration"
  echo "13. cargo test --locked --test wave2c_cli_integration"
  echo "14. cargo test --locked --test mvp_gate_baseline"
  echo "15. cargo test --locked --test doctor_parity_fixtures"
  echo

  if [[ ${overall_status} -eq 0 ]]; then
    echo "### Result: ✅ PASS"
    echo
    echo "Pass criteria met:"
    echo "- No runtime/setup failures"
    echo "- No contract drift in library tests, contract fixtures, golden_corpus_gate, tier_a_golden, integration, or wave2c_cli_integration"
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
run_step runtime "Linting" cargo clippy --locked --all-targets -- -D warnings

run_step contract "Library tests" cargo test --locked --lib
run_step contract "Contract fixtures" cargo test --locked --test contract_fixtures
run_step contract "Contract validation fixtures" cargo test --locked --test contract_validation_fixtures
run_step contract "Contract rules regression" cargo test --locked --test contracts_rules_contract_refs
run_step contract "Structured diagnostics regression" cargo test --locked --test structured_diagnostics_contracts
run_step contract "Contract E2E" cargo test --locked --test contract_e2e
run_step contract "Contract E2E edge" cargo test --locked --test contract_e2e_edge
run_step contract "Golden corpus" cargo test --locked --test golden_corpus_gate
run_step contract "Tier A deterministic gate" cargo test --locked --test tier_a_golden
run_step contract "Integration semantics" cargo test --locked --test integration
run_step contract "Wave2-C CLI integration semantics" cargo test --locked --test wave2c_cli_integration

run_step policy "Baseline/new-violation behavior" cargo test --locked --test mvp_gate_baseline
run_step contract "Doctor parity fixtures" cargo test --locked --test doctor_parity_fixtures

emit_summary

exit ${overall_status}
