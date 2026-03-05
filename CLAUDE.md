# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What Is Specgate

Rust CLI that enforces module boundary policy for TS/JS projects via `.spec.yml` files. Produces deterministic, byte-identical JSON verdicts for CI reliability.

## Build & Test Commands

```bash
# Build
cargo build --locked

# Full CI gate (run before committing)
./scripts/ci/mvp_gate.sh

# Individual steps
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --lib          # unit tests only (fast)
cargo test --locked                # all tests (slow)
cargo test --locked <test_name>    # single test by substring

# Key integration test suites
cargo test --locked --test contract_fixtures
cargo test --locked --test tier_a_golden
cargo test --locked --test integration

# npm wrapper (in npm/specgate/)
cd npm/specgate && npm run check && npm test
```

## Architecture

**Entry point:** `src/main.rs` → `specgate::cli::run()` → `CliRunResult` (stdout + stderr + exit code)

**Data flow for `specgate check`:**
1. `spec/config.rs` — parse `specgate.config.yml` → `SpecConfig`
2. `spec/mod.rs` — discover and parse `.spec.yml` files → `Vec<SpecFile>`
3. `graph/discovery.rs` — discover source files
4. `parser/mod.rs` — OXC-based AST parse → `FileAnalysis` per file
5. `resolver/mod.rs` — `ModuleResolver` (oxc_resolver + tsconfig) → `ResolvedImport`
6. `graph/mod.rs` — build petgraph `DependencyGraph`
7. `rules/` — evaluate all rule families → `Vec<RuleViolation>`
8. `baseline/mod.rs` — classify violations against baseline (new vs known)
9. `verdict/mod.rs` — `build_verdict_with_options()` → deterministic JSON

**CLI modules** (`src/cli/`): `mod.rs` has clap Parser + dispatch, `check.rs`/`doctor.rs`/`init.rs`/`validate.rs` have per-command args and handlers.

**Rule engine** (`src/rules/`): Two traits — `Rule` (stateless, graph-only) and `RuleWithResolver` (needs resolver access). Rule families: `boundary.rs`, `circular.rs`, `contracts.rs`, `dependencies.rs`, `layers.rs`.

**npm package** (`npm/specgate/`): Binary forwarding shim (spawns native Rust binary) + TypeScript Compiler API resolution snapshot generator for `doctor compare`.

## Key Conventions

- **Determinism:** `BTreeMap`/`BTreeSet` everywhere (never `HashMap` for output). Paths normalized via `deterministic/mod.rs`. Default output mode omits timestamps.
- **Exit codes are contractual:** 0=pass, 1=violations, 2=config error, 3=doctor mismatch
- **Spec versions:** exact string match `"2.2"` or `"2.3"` (not semver). `"2.3"` required for contracts.
- **Rule IDs:** dot notation (`boundary.allow_imports_from`, `dependency.forbidden`, `no_circular_deps`)
- **Module IDs:** slash-delimited strings (`api/orders`, `infra/db`)
- **Error handling:** `thiserror` for error types, `miette` for diagnostic rendering. CLI never panics.
- **Fingerprints:** SHA-256 via `stable_hash_hex()` — algorithm is contractual (changes invalidate baselines)

## Testing

Tests call `specgate::cli::run()` directly (no subprocess). Fixtures in `tests/fixtures/`. Tier A fixtures have `intro/` (must fail) and `fix/` (must pass) directories. Tests use `tempfile::TempDir` for isolation and `pretty_assertions` for diffs.

## Wave 0 Contract Lock

CLI interface, exit codes, output schema, baseline fingerprint format, and `--output-mode deterministic` output are locked per `WAVE0_CONTRACT.md`. Do not change without coordinating with consumers.

## CI/CD

- **Merge gate:** `mvp-merge-gate.yml` runs `./scripts/ci/mvp_gate.sh` on PR and push to master
- **Release:** tag `v*` triggers cross-compile for linux-x86_64, mac-x86_64, mac-arm64
- **npm publish:** manual workflow `release-npm-wrapper.yml`
- **Toolchain:** pinned to 1.88.0 via `rust-toolchain.toml`, MSRV 1.85.0
