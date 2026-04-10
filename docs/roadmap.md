# Specgate Roadmap

This is the current operator-facing source of truth for post-`v0.3.2` roadmap status. Historical plans stay in `docs/plans/` and `docs/archive/`. Active status belongs here.

## Landed

- Phase 5 envelope checks are shipped, including envelope AST validation and `match.pattern` function scoping.
- `specgate policy-diff` is shipped with deterministic `human`, `json`, and `ndjson` output plus `0/1/2` exit semantics.
- `policy-diff` semantic rename/copy pairing is shipped; inconclusive pairings remain fail-closed widening risk.
- `policy-diff` config-level governance diffing for `specgate.config.yml` is shipped and folds config changes into summary/net classification.
- `policy-diff --cross-file-compensation` is shipped as an opt-in, directly-connected-module compensation analysis with fail-closed ambiguity handling.
- `specgate check --deny-widenings` is shipped for in-band governance enforcement when `--since <base-ref>` is provided.
- `specgate check --format sarif` is shipped for SARIF 2.1.0 output in CI scanning workflows.
- `specgate doctor ownership` is shipped, including strict ownership gating for CI.
- Ownership-governance config semantics are shipped for `strict_ownership` and `strict_ownership_level`; today the runtime `doctor ownership` gate uses `strict_ownership_level: errors` for duplicate module ids and invalid ownership globs, and `strict_ownership_level: warnings` for all ownership findings.
- Monorepo support is shipped, including workspace discovery and nearest-tsconfig resolution behavior.
- Verification baseline is green in `docs/project-status.md` for `cargo test`, `policy_diff_integration`, `contract_fixtures`, `tier_a_golden`, `golden_corpus`, `cargo check --all-targets`, `cargo clippy --all-targets -- -D warnings`, and `cargo fmt --check`.

## Released

- `v0.3.2` is the current release cut from `master` after re-running the merge gate, verifying `policy-diff` against `origin/master`, building with `--release --locked`, smoke-checking the binary, and generating a SHA-256 checksum.

## Current Status

- The `v0.3.2` release-closeout work is complete.
- Remaining items below are genuine unimplemented Tier 2/Tier 3 backlog, not unfinished release-publication work.

## Remaining Tier 2 / Tier 3 Backlog

- Rule-family expansion remains incomplete for the future-facing `C02` pattern-aware variants, `C06` category-level governance checks, and `C07` unique-export / visibility edge cases still called out in operator docs.
- `doctor governance-consistency` / contradictory namespace-intent detection from the Tier 2 planning set is not shipped.
- Additional ownership/path hardening beyond invalid-glob reporting, such as contradictory glob detection, remains unimplemented.
- Import-hygiene coverage still has backlog items beyond the shipped rule/config surface, including the deeper package-internal scenarios noted in adversarial docs.
