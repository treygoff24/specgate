# Specgate P2 Policy Governance Build Checklist

Repo: `~/Development/specgate`
Branch: `p2-policy-governance`
Plan: `docs/plans/p2-policy-governance.md`

- [x] Task 1: Policy domain scaffolding + core types (`src/policy/mod.rs`, `src/policy/types.rs`, `src/lib.rs`, tests)
- [x] Task 2: Git ref validation, shallow clone handling, and change discovery (`src/policy/git.rs`, tests)
- [x] Task 3: Batched blob loader + parsed snapshot builder (`src/policy/git.rs`, tests)
- [x] Task 4: Field-level semantic classifier (`src/policy/classify.rs`, tests)
- [x] Task 5: `boundaries.path` coverage comparator (`src/policy/classify.rs`, `src/policy/git.rs`, tests)
- [x] Task 6: Renderers (`src/policy/render.rs`, tests)
- [x] Task 7: CLI command wiring (`src/cli/policy_diff.rs`, `src/cli/mod.rs`, `src/cli/tests.rs`)
- [x] Task 8: Integration tests with adversarial git fixtures (`tests/policy_diff_integration.rs`, `tests/fixtures/policy_diff/**`)
- [x] Task 9: Documentation + CI guidance (`docs/reference/policy-diff.md`, `README.md`, `CHANGELOG.md`)
- [x] Review 1: Nous review of Task 1-5 core policy engine
- [x] Review 2: Nous review of Task 6-9 CLI/docs/integration
- [x] Final gate on `p2-policy-governance`: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && ./scripts/ci/mvp_gate.sh`
- [ ] Merge to master and push
