# Project Status: Release Closeout Verification

Last verified on 2026-04-10.

This page is the verification ledger for the current release line.

| Check | Role | Status | Result |
| --- | --- | --- | --- |
| `cargo test --locked` | Full repo suite | pass | Full suite green with 498 unit tests plus integration, fixture, contract, perf-budget, and doc-test coverage (0 failed). |
| `cargo run --quiet -- policy-diff --base origin/master --format json` | Governance drift verification | pass | Returned an empty diff (`modules_changed: 0`, `has_widening: false`, no errors) against `origin/master`. |
| `cargo test --locked --test perf_budget` | Performance budget verification | pass | `7 passed` (0 failed), including `single_root_tsconfig_perf_not_regressed` and monorepo budget coverage. |
| `cargo test --locked --test contract_fixtures` | Contract fixture verification | pass | `23 passed` (0 failed), including `check --deny-widenings` behavior and pattern-aware boundary fixtures. |
| `cargo test --test tier_a_golden` | Merge-gate fixture contract | pass | `1 passed` (0 failed). |
| `cargo test --test golden_corpus` | Informational future-proxy coverage | pass | `10 passed` (0 failed); this suite is green but remains informational rather than merge-blocking. |
| `./scripts/ci/mvp_gate.sh` | Enforced merge-gate sequence | pass | Passed the full 18-command merge gate, including locked clippy, library tests, contract regressions, Tier A, integration, and monorepo suites. |
| `cargo check --all-targets` | Compile verification | pass | Completed successfully; no check errors. |
| `cargo clippy --all-targets -- -D warnings` | Lint verification | pass | Completed successfully; no lint warnings or errors. |
| `cargo fmt --check` | Formatting verification | pass | Completed successfully; formatting already compliant. |
| `cargo build --release --locked` | Release build verification | pass | Optimized release binary built successfully from the `v0.3.2` release candidate state. |
| `./target/release/specgate --version` | Release smoke verification | pass | Reported `specgate 0.3.2`. |
| `shasum -a 256 target/release/specgate` | Release checksum verification | pass | Produced a deterministic SHA-256 checksum for the local release binary. |
| `npm --prefix npm/specgate run check` | npm wrapper syntax verification | pass | Wrapper entrypoints and helper scripts all passed `node --check`. |
| `npm --prefix npm/specgate test` | npm wrapper behavior verification | pass | Wrapper tests passed, including native forwarding, workspace discovery, support-matrix, and signal-handling coverage. |

All release-closeout verification checks above were re-run and verified green for `v0.3.2`.

Post-closeout shipped status:

- `policy-diff` now includes shipped config-level governance diffing and opt-in cross-file compensation, covered in-code by dedicated suites such as `tests/policy_diff_config.rs`, `tests/policy_diff_compensation.rs`, and related integration coverage.
- `strict_ownership_level` is present in `specgate.config.yml`, participates in config-governance classification, and now affects `doctor ownership` gating: `errors` gates duplicate module ids and invalid ownership globs, while `warnings` gates all ownership findings.
- Fresh 2026-04-10 verification reran `cargo test --locked`, `cargo test --locked --test perf_budget`, `cargo run --quiet -- policy-diff --base origin/master --format json`, `./scripts/ci/mvp_gate.sh`, `cargo build --release --locked`, and the npm wrapper checks, and all passed.
