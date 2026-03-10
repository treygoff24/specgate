# Specgate Remaining Buildout Handoff

Last updated: 2026-03-10

> **Historical note:** This handoff was written before `v0.3.0` release
> publication completed. Current repo truth now lives in `docs/roadmap.md`,
> `docs/project-status.md`, `README.md`, and `SPECGATE_FOR_AGENTS.md`.

## Purpose

Use this file as historical context for what remained before the `v0.3.0`
release was cut from `master`.

This handoff is intentionally split into:

1. Release-publication work that was open at the time.
2. Explicitly deferred product backlog that was not built yet.
3. Historical backlog ideas that should only be revived deliberately.

## Already Shipped

Do not spend a fresh session rebuilding these unless you find a concrete regression:

- Phase 5 envelope checks, including `match.pattern` scoping.
- `specgate policy-diff`.
- Semantic rename/copy pairing in `policy-diff`.
- `specgate check --deny-widenings`.
- SARIF output via `specgate check --format sarif`.
- `specgate doctor ownership` plus `strict_ownership: true`.
- Monorepo support and nearest-tsconfig resolution.
- CLI refactor / doctor modularization.
- Current docs alignment for release-closeout status.

Primary evidence:

- [docs/roadmap.md](../roadmap.md)
- [docs/project-status.md](../project-status.md)
- [README.md](../../README.md)

## Release Work That Was Open At The Time

At the time this handoff was written, these were the only release-publication
items still open in the active roadmap.

### 1. Release publication

Status at handoff time: not a product-build task, but still required to call the release complete.

Source:

- [docs/roadmap.md](../roadmap.md)
- [RELEASING.md](../../RELEASING.md)

What remains:

- Re-run release gates on the exact release commit.
- Build release binaries with `--locked`.
- Smoke-check the release binary.
- Generate checksums.
- Cut the release tag.
- Push the tag and publish release notes/artifacts.

Current state:
- Completed on 2026-03-10 as `v0.3.0`.

Suggested commands:

```bash
./scripts/ci/mvp_gate.sh
specgate policy-diff --base origin/master
cargo build --release --locked
./target/release/specgate --version
shasum -a 256 ./target/release/specgate
git tag -a vX.Y.Z -m "Specgate vX.Y.Z"
git push origin master --tags
```

Important:

- Do not treat repo-root `specgate check` or `doctor ownership` runs as release blockers for the Specgate repo itself; `tests/fixtures/` intentionally contains invalid and duplicate specs for contract coverage.
- Use exactly one governance gate in release/CI: `policy-diff` or `check --deny-widenings`.

## Explicitly Deferred Product Backlog

These are the only clearly active not-built feature items called out in the roadmap.

### 2. Cross-file compensation in `policy-diff`

Status: explicitly deferred beyond this release.

Source:

- [docs/roadmap.md](../roadmap.md)
- [docs/reference/policy-diff.md](../reference/policy-diff.md)

Problem:

- Today a widening in one `.spec.yml` file is not offset by a narrowing in another file.

Likely files:

- `src/policy/classify.rs`
- `src/policy/mod.rs`
- `src/policy/types.rs`
- `src/policy/tests.rs`
- `tests/policy_diff_integration.rs`
- `docs/reference/policy-diff.md`
- `docs/roadmap.md`

Minimum acceptance:

- Multi-file diff sets can classify a net-safe change without silently masking unrelated widenings.
- Behavior stays deterministic and fail-closed when compensation is ambiguous.

Verification:

```bash
cargo test --test policy_diff_integration
cargo test
```

### 3. Config-level governance diffing for `specgate.config.yml`

Status: explicitly deferred beyond this release.

Source:

- [docs/roadmap.md](../roadmap.md)
- [docs/reference/policy-diff.md](../reference/policy-diff.md)

Problem:

- Governance diffing currently ignores `specgate.config.yml`.

Likely files:

- `src/policy/git.rs`
- `src/policy/classify.rs`
- `src/policy/types.rs`
- `src/policy/render.rs`
- `src/policy/tests.rs`
- `tests/policy_diff_integration.rs`
- `docs/reference/policy-diff.md`

Minimum acceptance:

- Clearly defined semantics for governance-relevant config changes.
- Deterministic output and safe failure behavior.
- Explicit documentation for what config fields count as widening, narrowing, or structural.

Verification:

```bash
cargo test --test policy_diff_integration
cargo test
```

### 4. Deferred future rule-family expansion

Status: explicitly deferred beyond this release.

Source:

- [docs/roadmap.md](/Users/treygoff/Code/specgate/docs/roadmap.md)
- [docs/reference/operator-guide.md](/Users/treygoff/Code/specgate/docs/reference/operator-guide.md)

Deferred examples already called out in docs:

- `C02` pattern-aware variants
- `C06` category-level governance variants
- `C07` unique-export / visibility edge-case variants

Likely files:

- `tests/fixtures/golden/**`
- `tests/golden_corpus.rs`
- `tests/tier_a_golden.rs`
- matching operator/docs references

Minimum acceptance:

- New fixtures exist with explicit expected outcomes.
- Docs say whether each new family is merge-gating or informational.
- No nondeterministic snapshots.

Verification:

```bash
cargo test --test golden_corpus
cargo test --test tier_a_golden
cargo test
```

## Historical Backlog To Revive Only On Purpose

These came from older hardening planning and should not be treated as active release scope unless you intentionally decide to build them.

Source:

- [docs/plans/hardening-phase.md](../plans/hardening-phase.md)

Potential candidates:

- Contradictory glob detection in ownership validation.
- Richer provider-side visibility / allow-consumer model.

Do not assume these are approved next steps just because they appear in historical plans.

## Recommended Fresh-Session Order

If the goal is "finish everything still active":

1. Do the release-publication work first.
2. If you want post-release product work, pick one deferred governance item next:
   - cross-file compensation, or
   - config-level governance diffing.
3. Do deferred fixture/rule-family expansion after governance semantics are settled.

If the goal is "build every remaining plausible backlog item":

1. Finish release publication.
2. Open a spec/planning pass that decides which historical backlog items are still approved.
3. Only then implement new product scope.

## Verification Baseline

These commands are the current baseline for any serious follow-on work:

```bash
cargo test
cargo check --all-targets
cargo clippy --all-targets -- -D warnings
cargo fmt --check
./scripts/ci/mvp_gate.sh
```

## Suggested Fresh-Session Prompt

```text
Read docs/handoffs/2026-03-10-remaining-buildout-handoff.md and continue from there.
Treat docs/roadmap.md as the source of truth for active remaining scope.
Do not rebuild already-shipped closeout items.
Start with the highest-priority remaining item and carry it through implementation, verification, and doc updates.
```
