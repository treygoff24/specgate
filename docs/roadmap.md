# Specgate Roadmap

This is the current operator-facing source of truth for release-closeout status. Historical plans stay in `docs/plans/` and `docs/archive/`, but active status belongs here.

## Landed

- Phase 5 envelope checks are shipped, including envelope AST validation and `match.pattern` function scoping.
- `specgate policy-diff` is shipped with deterministic `human`, `json`, and `ndjson` output plus `0/1/2` exit semantics.
- `policy-diff` semantic rename/copy pairing is shipped; inconclusive pairings remain fail-closed widening risk.
- `specgate check --deny-widenings` is shipped for in-band governance enforcement when `--since <base-ref>` is provided.
- `specgate check --format sarif` is shipped for SARIF 2.1.0 output in CI scanning workflows.
- `specgate doctor ownership` is shipped, including strict ownership gating for CI.
- Monorepo support is shipped, including workspace discovery and nearest-tsconfig resolution behavior.
- Verification baseline is green in `docs/project-status.md` for `cargo test`, `policy_diff_integration`, `contract_fixtures`, `tier_a_golden`, `golden_corpus`, `cargo check --all-targets`, `cargo clippy --all-targets -- -D warnings`, and `cargo fmt --check`.

## In Progress

- Release publication remains: re-run gates on the release commit, cut the tag, and publish notes/artifacts.

## Remaining to Call This Release Complete

- Re-run release gates on the release commit, cut the release tag, and publish release notes from the aligned docs.

## Explicitly Deferred Beyond This Release

- Cross-file compensation in `policy-diff` remains deferred (a widening in one file is not offset by narrowing in another).
- Config-level governance diffing for `specgate.config.yml` is deferred-by-decision for this release and remains out of scope for `policy-diff`.
- Deferred future rule-variant expansion in fixture coverage remains outside this release scope (for example, pattern-aware and category-level variants called out in operator docs).
