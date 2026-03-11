# Project Status: Release Closeout Verification

Last verified on 2026-03-11.

| Check | Role | Status | Result |
| --- | --- | --- | --- |
| `cargo test` | Full repo suite | pass | Full suite green with 377 unit tests plus integration and fixture suites (0 failed). |
| `cargo test --test policy_diff_integration` | Governance contract verification | pass | `18 passed` (0 failed), including semantic rename/copy pairing and fail-closed widening checks. |
| `cargo test --test contract_fixtures` | Contract fixture verification | pass | `15 passed` (0 failed), including `check --deny-widenings` behavior. |
| `cargo test --test tier_a_golden` | Merge-gate fixture contract | pass | `1 passed` (0 failed). |
| `cargo test --test golden_corpus` | Informational future-proxy coverage | pass | `10 passed` (0 failed); this suite is green but remains informational rather than merge-blocking. |
| `./scripts/ci/mvp_gate.sh` | Enforced merge-gate sequence | pass | Passed the full 18-command merge gate, including locked clippy, library tests, contract regressions, Tier A, integration, and monorepo suites. |
| `cargo check --all-targets` | Compile verification | pass | Completed successfully; no check errors. |
| `cargo clippy --all-targets -- -D warnings` | Lint verification | pass | Completed successfully; no lint warnings or errors. |
| `cargo fmt --check` | Formatting verification | pass | Completed successfully; formatting already compliant. |
| Release publication (`v0.3.0`) | Tag + push verification | pass | Release commit `5fd3079` was tagged as `v0.3.0` and pushed to `origin`; locked release binary smoke-check passed and SHA-256 checksum was generated. |

All release-closeout verification checks above were re-run and verified green, and the `v0.3.0` release was cut from the verified commit.

Post-closeout shipped-status note:

- `policy-diff` now includes shipped config-level governance diffing and opt-in cross-file compensation, covered in-code by dedicated suites such as `tests/policy_diff_config.rs`, `tests/policy_diff_compensation.rs`, and related integration coverage.
- `strict_ownership_level` is present in `specgate.config.yml`, participates in config-governance classification, and now affects `doctor ownership` gating: `errors` gates duplicate module ids and invalid ownership globs, while `warnings` gates all ownership findings.
- Fresh 2026-03-11 verification reran `cargo test --locked`, `cargo test --locked --test ownership_validation`, and `./scripts/ci/mvp_gate.sh`, and all passed.
