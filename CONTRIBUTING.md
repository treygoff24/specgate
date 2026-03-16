# Contributing to Specgate

Thanks for your interest in contributing. This document covers the basics.

## Filing issues

Open a GitHub issue for bugs, feature requests, or questions. Include:

- Specgate version (`specgate --version` or `cargo install` tag)
- OS and architecture
- Minimal reproduction steps (a small repo or spec file that triggers the problem)
- Expected vs actual behavior

For boundary-rule false positives, include the spec file, the relevant source files, and the `specgate check` output.

## Pull requests

1. Fork the repo and create a branch from `master`.
2. Make your changes. Write tests for new behavior.
3. Run the full verification suite before submitting:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
./scripts/ci/mvp_gate.sh
```

4. Open a PR against `master`. Describe what changed and why.

## Code style

- **Rust edition 2024**, MSRV 1.85+.
- `cargo fmt` with default settings. No custom rustfmt config.
- `cargo clippy --all-targets -- -D warnings` must pass clean.
- Prefer explicit types over inference when the type isn't obvious from context.
- Error handling: propagate with `?` and `anyhow`. Add context with `.context("what failed")`.

## Tests

All PRs must pass `cargo test`. The test suite has several layers:

- **Unit tests** in `src/` modules
- **Integration tests** in `tests/`
- **Contract fixtures** (`tests/contract_fixtures.rs`) for boundary contract validation
- **Golden corpus** (`tests/golden_corpus.rs`) for deterministic regression testing
- **Tier A fixtures** (`tests/tier_a_golden.rs`) for the merge gate contract

If you're adding a new rule, add both a positive test (violation detected) and a negative test (clean code passes). Add a golden fixture if the rule affects verdict output.

## Commit messages

Use conventional commit format:

```
feat(rules): add C08 re-export depth limit rule
fix(resolver): handle NodeNext .mjs extension aliases
docs(operator): clarify baseline refresh workflow
test(golden): add Tier A fixture for category governance
chore(ci): update release-binaries action versions
```

## Architecture overview

Specgate is a single Rust crate with internal module boundaries:

- `src/spec/` — Spec file parsing, validation, and types
- `src/rules/` — Boundary, dependency, and hygiene rule evaluation
- `src/policy/` — Governance tracking, policy diffing, git integration
- `src/cli/` — Command handlers and output formatting
- `src/resolver/` — Import resolution via oxc-resolver
- `src/parser/` — TypeScript/JavaScript AST parsing via oxc
- `src/verdict/` — Verdict construction, formatting, and SARIF output
- `src/baseline/` — Baseline fingerprinting and audit
- `src/graph/` — Module dependency graph construction

## Agent contributors

AI agents are welcome contributors. Point your agent at [SPECGATE_FOR_AGENTS.md](SPECGATE_FOR_AGENTS.md) for the onboarding path, then follow the same PR process as human contributors. The merge gate (`./scripts/ci/mvp_gate.sh`) is the source of truth for whether a change is shippable.

## License

By contributing, you agree that your contributions will be licensed under the MIT License.
