# MVP Merge Gate (CI)

This repository defines a single **MVP-ready merge gate** for contract-sensitive changes.

- Workflow: `.github/workflows/mvp-merge-gate.yml`
- Runner script: `scripts/ci/mvp_gate.sh`

## Required Gate Command Sequence

The gate runs this exact sequence:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --test contract_fixtures
cargo test --test golden_corpus
cargo test --test tier_a_golden
cargo test --test mvp_gate_baseline
```

## Pass Criteria

The gate passes only when **all** commands above pass.

Additionally, pass/fail is categorized in CI logs and step summary as:

- **Runtime/setup failure**
  - Formatting, linting, toolchain, or command execution failures.
- **Contract drift**
  - Any failure in:
    - `contract_fixtures`
    - `golden_corpus`
    - `tier_a_golden`
- **Policy failure**
  - Baseline behavior checks fail in `mvp_gate_baseline`.

## Baseline Behavior Covered by Gate

`mvp_gate_baseline` enforces both required baseline semantics:

1. Existing baseline violations are report-only (`check` exits 0).
2. New violations after baseline generation fail policy gate (`check` exits 1).

These checks are part of the required command sequence above.
