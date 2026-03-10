# Specgate Roadmap

This is the single operator-facing roadmap for release-closeout tracking. Historical plans stay in `docs/plans/` and `docs/archive/`, but active status belongs here.

## Landed

- Phase 5 envelope checks are shipped, including envelope AST validation and `match.pattern` function scoping.
- `specgate policy-diff` is shipped with deterministic `human`, `json`, and `ndjson` output plus `0/1/2` exit semantics.
- `specgate check --format sarif` is shipped for SARIF 2.1.0 output in CI scanning workflows.
- `specgate doctor ownership` is shipped, including strict ownership gating for CI.
- Monorepo support is shipped, including workspace discovery and nearest-tsconfig resolution behavior.
- Verification baseline is green in `docs/project-status.md` for `cargo test`, `policy_diff_integration`, `contract_fixtures`, `tier_a_golden`, and `cargo fmt --check`.

## In Progress

- Closeout narrative alignment across operator-facing docs is in progress so README, operator guide, and reference docs point to one roadmap source.
- Adoption hardening is in progress for canonical CI examples and release-readiness documentation alignment.
- Final quality-gate closeout is in progress (review and post-fix gates) before this release is declared complete.

## Remaining to Call This Release Complete

- Implement `specgate check --deny-widenings` so policy widening can be enforced directly in `check`.
- Add semantic rename/copy pairing for `.spec.yml` in `policy-diff` so equivalent renames are no longer fail-closed widenings.
- Publish final release closeout notes after full gate revalidation and doc reconciliation are complete.

## Explicitly Deferred Beyond This Release

- Cross-file compensation in `policy-diff` remains deferred (a widening in one file is not offset by narrowing in another).
- Config-level governance diffing for `specgate.config.yml` remains out of scope for `policy-diff` in this release.
- Deferred future rule-variant expansion in fixture coverage remains outside this release scope (for example, pattern-aware and category-level variants called out in operator docs).
