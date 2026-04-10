# MVP Merge Gate (CI)

This document defines what "safe to merge" means for Specgate itself.

The repository has one merge-ready gate for contract-sensitive changes.

- Workflow: `.github/workflows/mvp-merge-gate.yml`
- Runner script: `scripts/ci/mvp_gate.sh`

---

## What the Gate Must Prove

A change is merge-ready only when all required checks pass and are categorized consistently:

1. **Runtime/setup health**
   - Formatting/lint/tooling commands execute successfully.
2. **Wave 0 + deterministic contract stability**
  - `contract_fixtures`, `golden_corpus_gate`, and `tier_a_golden` stay green.
3. **Baseline/new-violation policy semantics**
  - Baseline hits remain report-only; newly introduced violations are merge-blocking.

`golden_corpus` remains informational and is used as future-proxy coverage for deferred rules not in the enforced merge gate.

## Gate taxonomy (for this repo)

- **Gating:** every command in the exact `scripts/ci/mvp_gate.sh` sequence below must pass for CI merge.
- **Informational:** `golden_corpus` (`tests/golden_corpus.rs`) tracks future-proxy coverage and is not enforced by merge gate.
- **Informational:** diagnostics (`doctor compare`), metrics-mode tuning, and non-gating fixture experiments.

---

## Required Gate Command Sequence

The gate runs this exact sequence:

```bash
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --lib
cargo test --locked --test contract_fixtures
cargo test --locked --test contract_validation_fixtures
cargo test --locked --test contracts_rules_contract_refs
cargo test --locked --test structured_diagnostics_contracts
cargo test --locked --test contract_e2e
cargo test --locked --test contract_e2e_edge
cargo test --locked --test golden_corpus_gate
cargo test --locked --test tier_a_golden
cargo test --locked --test integration
cargo test --locked --test wave2c_cli_integration
cargo test --locked --test mvp_gate_baseline
cargo test --locked --test doctor_parity_fixtures
cargo test --locked --test tsjs_barrel_fixtures
cargo test --locked --test tsjs_openclaw_regression
cargo test --locked --test monorepo_integration
```

---

## Pass/Fail Criteria and Failure Mapping

The gate passes only when **all** commands above pass.

Failures are reported as one of:

- **Runtime/setup failure**
  - Formatting, linting, toolchain, or command execution failures.
- **Contract drift**
  - Any failure in the library tests, contract fixture/regression suites, `golden_corpus_gate`, `tier_a_golden`, `integration`, `wave2c_cli_integration`, `doctor_parity_fixtures`, `tsjs_barrel_fixtures`, `tsjs_openclaw_regression`, or `monorepo_integration`.
- **Policy failure**
  - Baseline behavior checks fail in `mvp_gate_baseline`.

---

## Baseline Behavior Covered by Gate

`mvp_gate_baseline` enforces both required semantics:

1. Existing baseline violations are report-only (`check` exits `0`).
2. New violations after baseline generation fail policy gate (`check` exits `1`).

---

## Related Docs

- [Operator Guide](operator-guide.md)
- [CI Gate Understanding](../design/ci-gate-understanding.md)
- [Historical Wave 0 Contract](../archive/status/WAVE0_CONTRACT.md)
- [Tier A Fixture Design](../design/tier-a-fixture-design.md)
- [Roadmap](../roadmap.md#current-status)
- [Baseline Policy](../design/baseline-policy.md)
- [Dogfood Rollout Checklist](../dogfood/rollout-checklist.md)
- [Releasing Guide](../../RELEASING.md)
