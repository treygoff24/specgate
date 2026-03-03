# Boundary Contracts V2 Build Report

Date: 2026-03-03
Repo: `specgate`
Branch: `master`
Build window: `5712d0d..3be42a8`

## Executive Summary

This build implemented the full **Boundary Contracts V2** scope from `docs/specgate-boundary-contracts-v2.md`, including spec language updates, contract validation/enforcement rules, check-path integration, structured diagnostics, output format expansion, regression suites, end-to-end fixtures, and CI gate coverage.

Final outcome:

- Boundary contracts are supported in spec version `2.3` with backward compatibility for `2.2`.
- Contract violations are validated and enforced deterministically.
- `--since` behavior includes contract checks with evaluation-time affected-module scoping.
- Structured diagnostics fields are surfaced in verdict output.
- `check` supports `--format human|json|ndjson` with TTY-aware defaults.
- CI gate script includes all new contract-focused suites.
- Full gate is green.

## Goals and Scope

Primary goals delivered:

1. Add boundary contract syntax/types and versioning behavior.
2. Validate contracts statically at spec load/validate time.
3. Enforce contracts in analysis/check path.
4. Add structured diagnostics metadata for contract/layer violations.
5. Extend output formatting for human + ndjson use cases.
6. Add deterministic test coverage from unit to integration/e2e.
7. Ensure CI merge gate catches contract regressions.

## What Changed

### 1) Spec Model + Versioning

Files:

- `src/spec/types.rs`
- `src/cli/mod.rs` (init scaffold)

Delivered:

- Added multi-version support constants (accepted versions include `2.2` and `2.3`).
- Added current scaffold version usage for `init`.
- Added contract-related spec model types and defaults.
- Ensured generated scaffold includes explicit `boundaries.contracts: []`.

### 2) Validation Layer

File:

- `src/spec/validation.rs`

Delivered:

- Added contract validation rule IDs to known constraint set.
- Added version gate: contracts in `2.2` trigger `boundary.contract_version_mismatch`.
- Added validation for:
  - contract id uniqueness/non-empty
  - contract path non-empty and extension validity
  - `match.files` non-empty + glob validity (`literal_separator(true)`)
  - `imports_contract` strict format checks

### 3) Contract Rules Engine

Files:

- `src/rules/contracts.rs`
- `src/rules/mod.rs`

Delivered:

- Implemented contract enforcement rules:
  - `boundary.contract_missing`
  - `boundary.contract_empty`
  - `boundary.match_unresolved`
  - `boundary.contract_ref_invalid`
- Added contract registry + cross-module contract reference checks.
- Added evaluation-time scoping support (`affected_modules`) for `--since` path.
- Exported contracts rule module/types from rules mod.

### 4) CLI/Analysis Integration

File:

- `src/cli/mod.rs`

Delivered:

- Integrated contract rule results into analysis pipeline and policy violation stream.
- Mapped contract violations as `Severity::Error` and made them baseline-compatible.
- Threaded affected-module scoping through check path.
- Preserved post-filter behavior for non-contract violations.

### 5) Structured Diagnostics

Files:

- `src/verdict/mod.rs`
- `src/cli/mod.rs`
- `src/verdict/format.rs`
- `src/baseline/mod.rs`

Delivered:

- Added optional fields to policy/verdict violations:
  - `expected`
  - `actual`
  - `remediation_hint`
  - `contract_id`
- Wired population behavior:
  - contract violations include `remediation_hint` and `contract_id`
  - layer violations include `remediation_hint`
  - non-contract violations keep `None`

### 6) Output Format Expansion

Files:

- `src/cli/check.rs`
- `src/cli/mod.rs`
- `src/verdict/format.rs`

Delivered:

- Added `--format` support (`human`, `json`, `ndjson`).
- Added TTY-aware effective format selection.
- Added human formatter and ndjson formatter.
- Added verdict schema marker field for output evolution.

### 7) New Tests + Fixtures

Files:

- `tests/contract_validation_fixtures.rs`
- `tests/contracts_rules_contract_refs.rs`
- `tests/structured_diagnostics_contracts.rs`
- `tests/contract_e2e.rs`
- `tests/contract_e2e_edge.rs`
- `tests/fixtures/contract-validation/*`
- `tests/fixtures/contract-project/*`
- `tests/fixtures/contract-project-edge/*`

Delivered:

- Added validation fixture coverage.
- Added rule-level regression coverage.
- Added structured diagnostics regression coverage.
- Added E2E coverage for happy path + edge cases + baseline behavior + `--since` behavior.

### 8) Golden + Gate Updates

Files:

- `scripts/ci/mvp_gate.sh`
- `tests/golden_corpus_gate.rs`
- `tests/fixtures/golden/contract-2.3.spec.yml`

Delivered:

- Added contract suites to MVP merge gate sequence.
- Added golden fixture for 2.3 contract spec validation and corresponding gate assertion.

## Wave/Commit Timeline (High Signal)

- `f9d6ad1` — init scaffold updates
- `daef182` — spec type foundations
- `d5ae4bf` — version gating validation
- `96ccaf2` — contract validation rules
- `2e0b0d4` — contract validation fixtures
- `18e24ff` / `7c677e1` — contract rules engine
- `6488edb` / `4711246` — contract rules regression tests + fixes
- `bcd2e45` — CLI integration for contract rules + scoping
- `47ffd3c` — structured diagnostics fields
- `24c156f` / `8c69c68` — structured diagnostics regression tests + clippy fixes
- `cbf890e` — human + ndjson formatters
- `600c744` / `73b00e2` — verdict schema versioning + test updates
- `cc9a7ce` / `6ab5216` — check format dispatch + lint cleanup
- `72f09d4` / `5f51b7f` — contract e2e tests + polish
- `d6b8f42` / `873544e` — contract edge/since tests + lint cleanup
- `3be42a8` — final gate + blast-radius regression fix + golden/gate updates

## Issues Encountered and Fixes

### 1) Recovery after interrupted run

- Build continuation required state reconstruction from git/worktree history.
- Resolved by reconciling actual branch state with planned wave status and continuing forward only.

### 2) Merge friction in wave transition

- A merge conflict cycle occurred around W7 merge handoff.
- Resolved by normalizing local formatting and re-merging cleanly.

### 3) `--since` integration regression

- Symptom: integration tests failed (`check_since_*` expectations dropped to zero violations).
- Root cause: blast radius was being computed with empty edge pairs in check paths.
- Fix: added pre-analysis graph edge derivation and passed real edge pairs into blast-radius computation in `src/cli/mod.rs`.
- Verification: failing integration tests passed after fix.

## Verification and Gate Evidence

Final full gate command:

```bash
./scripts/ci/mvp_gate.sh
```

Result: PASS.

This includes:

- `cargo fmt --check`
- `cargo clippy --locked --all-targets -- -D warnings`
- `cargo test --locked --lib`
- contract fixture/validation/rules/diagnostics suites
- contract e2e + contract edge e2e suites
- `golden_corpus_gate`, `tier_a_golden`, `integration`, `wave2c_cli_integration`, `mvp_gate_baseline`

## Build Footprint

Range stats (`5712d0d..3be42a8`):

- 39 files changed
- ~5900 insertions
- ~46 deletions

The high insertion count is expected due to new contract rule engine logic and new fixture-heavy test suites.

## Final State / Handoff Notes

- Branch `master` includes all Boundary Contracts V2 build work.
- Contract capability is now covered by layered tests from unit to full integration.
- CI merge gate now explicitly protects this behavior surface.
- No known open escalations remain from this build.

If you want to audit quickly, start with these files:

- `src/spec/types.rs`
- `src/spec/validation.rs`
- `src/rules/contracts.rs`
- `src/cli/mod.rs`
- `src/cli/check.rs`
- `src/verdict/mod.rs`
- `src/verdict/format.rs`
- `scripts/ci/mvp_gate.sh`
