# MVP Merge Gate (CI)

**One operator-facing definition of “safe to merge” for Specgate.**

This repository defines a single MVP-ready gate for contract-sensitive changes.

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

`golden_corpus` remains informational and is used as future-proxy coverage for rules not yet fully enforced.

## Gate taxonomy (for this repo)

- **Gating:** all commands in this document's sequence (`contract_fixtures`, `golden_corpus_gate`, `integration`, `wave2c_cli_integration`, `tier_a_golden`, `mvp_gate_baseline`) must pass for CI merge.
- **Informational:** `golden_corpus` (`tests/golden_corpus.rs`) tracks future-proxy coverage and is not enforced by merge gate.
- **Informational:** diagnostics (`doctor compare`), metrics-mode tuning, and non-gating fixture experiments.

---

## Required Gate Command Sequence

The gate runs this exact sequence:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --test contract_fixtures
cargo test --test golden_corpus_gate
cargo test --test tier_a_golden
cargo test --test integration
cargo test --test wave2c_cli_integration
cargo test --test mvp_gate_baseline
```

---

## Pass/Fail Criteria and Failure Mapping

The gate passes only when **all** commands above pass.

Failures are reported as one of:

- **Runtime/setup failure**
  - Formatting, linting, toolchain, or command execution failures.
- **Contract drift**
  - Any failure in `contract_fixtures`, `golden_corpus_gate`, `integration`, `wave2c_cli_integration`, or `tier_a_golden`.
- **Policy failure**
  - Baseline behavior checks fail in `mvp_gate_baseline`.

---

## Baseline Behavior Covered by Gate

`mvp_gate_baseline` enforces both required semantics:

1. Existing baseline violations are report-only (`check` exits `0`).
2. New violations after baseline generation fail policy gate (`check` exits `1`).

---

## Related Docs

- [Operator Guide](OPERATOR_GUIDE.md)
- [CI Gate Understanding](CI-GATE-UNDERSTANDING.md)
- [Wave 0 Contract](../WAVE0_CONTRACT.md)
- [Tier A Fixture Design](tier-a-fixture-design-v1.md)
- [Implementation Plan §15](specgate-implementation-plan-v1.1.md#15-remaining-work-prioritized)
