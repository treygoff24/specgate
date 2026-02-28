# Code Review Report: `codex/tsjs-v1-scale-foundation`

**Branch:** `codex/tsjs-v1-scale-foundation` vs `master`
**Scope:** 24 files, ~2,455 lines added/changed across 4 commits
**Reviewed:** 2026-02-28
**Verdict:** **Revise** (no critical blockers, but several high-severity issues warrant fixes before merge)

---

## Executive Summary

This branch adds TS/JS v1 distribution infrastructure: an npm wrapper package with TypeScript resolution snapshot generation, CI/CD workflows for npm publishing and perf budgets, Rust CLI features (baseline refresh, stale baseline policy, telemetry opt-in, structured trace IO, parser modes), and supporting documentation/contracts.

The implementation is generally well-structured with good separation of concerns. The Rust core changes are solid, the CI workflows are thorough, and the test coverage for new features is decent. However, several issues need attention before merge.

**Top 5 items to fix:**
1. npm wrapper `spawnSync` swallows signal kills silently
2. `refresh_baseline` preserves stale provenance metadata instead of updating it
3. CI workflow accepts semver build metadata (`+build`) but npm rejects it
4. No tests exist for the npm wrapper package (435-line resolution generator)
5. `--tsc-command`/`--allow-shell` security-relevant flags are undocumented

---

## Findings by Severity

### Critical (2)

| ID | Domain | File | Finding |
|----|--------|------|---------|
| C1 | CI/CD | `release-npm-wrapper.yml:4-6` | Workflow fires on ALL `release: [published]` events including prereleases from `release-binaries.yml`. No gate checks whether binary artifacts exist before pushing to npm. An npm package could publish pointing at a partial/broken release. |
| C2 | CI/CD | `release-npm-wrapper.yml:70` | `package_dir` from `workflow_dispatch` is passed directly to `cd` and path construction without traversal validation. A caller with write access could pass `../../.github`. |

### High (8)

| ID | Domain | File:Line | Finding |
|----|--------|-----------|---------|
| H1 | npm | `specgate.js:60-70` | `spawnSync` logic has unreachable branch when process is killed by signal. `result.status` is `null` and `result.error` is `undefined`, so the function falls through to `return 1` with no message. Signal kills (including Ctrl-C) are silently swallowed. |
| H2 | Rust | `baseline/mod.rs:191-205` | `refresh_baseline` preserves stale `generated_from` metadata (tool_version, git_sha, config_hash) from existing baseline instead of updating them. Running `specgate baseline --refresh` after a tool upgrade leaves wrong provenance. |
| H3 | Rust | `cli/mod.rs:1798` | `serde_json::from_value` clones the full `Value` tree before attempting `StructuredTraceSnapshot` deserialization. O(N) extra allocation on large tsc trace files. |
| H4 | Rust | `cli/mod.rs:877-884` | `handle_check_with_diff` produces no machine-readable `governance` field when `stale_baseline_policy: fail` triggers non-zero exit, creating an API gap vs the JSON check path. |
| H5 | CI/CD | `release-npm-wrapper.yml:59` | Version extracted from tag includes build metadata. For `v1.0.0+build123`, version becomes `1.0.0+build123`. npm rejects `+` in version strings. Fix: `version="${version%%+*}"`. |
| H6 | CI/CD | `release-npm-wrapper.yml:158-177` | `Verify publish and dist-tag` step has no retry/backoff. npm registry propagation is not instantaneous; `npm view` immediately after publish can return 404. |
| H7 | npm | `generate-resolution-snapshot.js:280` | `fs.realpathSync.native` called unconditionally on `projectRoot` without `tryRealpath` wrapper. Throws raw `ENOENT` for non-existent paths instead of friendly error message. |
| H8 | Docs | `spec-language.md`, `WAVE0_CONTRACT.md` | `--tsc-command`/`--allow-shell` security-relevant CLI pair (executes arbitrary shell code via `sh -lc`) is completely absent from all documentation. |

### Medium (12)

| ID | Domain | File:Line | Finding |
|----|--------|-----------|---------|
| M1 | npm | `generate-resolution-snapshot.js:300` | Module resolution cache key function is identity `(v) => v`. Should be case-normalizing on case-insensitive file systems (macOS, Windows). Produces incorrect cache behavior. |
| M2 | npm | `package.json:30` | `typescript` is a hard `dependency` (~20MB) forced on all consumers, even those only using native binary forwarding. Should be `peerDependency` or lazy-required. |
| M3 | npm | `generate-resolution-snapshot.js:152-188` | Arg parser doesn't detect flag-as-value (e.g., `--from --import foo` silently uses `--import` as the `from` path). |
| M4 | Rust | `cli/mod.rs:620-627` | Telemetry event mutated into verdict post-construction. Coupling risk if verdict post-processing is added later. |
| M5 | Rust | `cli/mod.rs:2368-2389` | `result_kind` matched as raw `&str` strings. New values silently fall through to catch-all. Should be an enum. |
| M6 | Rust | `cli/mod.rs:2807-2813` | `project_fingerprint` uses `to_string_lossy` — breaks cross-platform hash reproducibility on non-UTF-8 paths. |
| M7 | CI/CD | `perf-tier1.yml` | No Rust build cache; cold compile + wall-clock timing test on shared CI runners is inherently non-reproducible. |
| M8 | CI/CD | `release-npm-wrapper.yml:17-19` | `id-token: write` permission at workflow level instead of job-scoped to `publish`. Over-grants OIDC to `resolve` job. |
| M9 | Docs | `DOGFOOD_RELEASE_CHANNEL.md` | Drops the prior semver upgrade policy (patch/minor/breaking definitions) with no replacement. CI-gating tool consumers need this contract. |
| M10 | Docs | `WAVE0_CONTRACT.md:93-94` | Lists `--structured-snapshot-out` but it's not documented in `spec-language.md`. |
| M11 | Docs | `DOGFOOD_RELEASE_CHANNEL.md:37` | "Gate" is undefined — doesn't specify which workflow(s) constitute the promotion gate. |
| M12 | Tests | `wave2c_cli_integration.rs` | Telemetry test mutates shared TempDir config mid-test across 4 sub-cases. Fragile if reordered or split. |

### Low (10)

| ID | Domain | File:Line | Finding |
|----|--------|-----------|---------|
| L1 | Rust | `baseline/mod.rs:196` | `refresh_baseline` runs classification walk twice (once to count stale, once in `build_baseline_with_metadata`). O(N) redundant work. |
| L2 | Rust | `cli/mod.rs` | `--refresh` flag help text says "pruning stale entries" but semantics are "rebuild from current violations". Misleading. |
| L3 | Rust | `cli/mod.rs:1799-1803` | `"1.0.0"` schema_version fallback has no removal target or deprecation warning. Migration shim will accumulate. |
| L4 | npm | `specgate.js:87` | `"snapshot-resolution"` alias is undocumented in help text and README. |
| L5 | npm | `specgate.js:25` | `SPECGATE_NATIVE_BIN` resolved relative to `process.cwd()`, not package root. Breaks if caller changes directory. |
| L6 | CI/CD | `perf_budget.sh:6-8` | Non-numeric env var values silently fall back to defaults. Should validate or fail fast. |
| L7 | CI/CD | `package.json:3` | `version: "0.1.0"` must be manually bumped before each release tag. No automation or reminder. |
| L8 | Tests | `mvp_gate_baseline.rs` | Missing negative test for `stale_baseline: warn` (default) not blocking the gate. |
| L9 | Tests | `perf_budget.rs` | Fixture has no constraints/violations — only stresses "clean" path, not policy evaluation. |
| L10 | Docs | `support-matrix-v1.md` | `aarch64-unknown-linux-gnu` (Linux ARM64/Graviton) not mentioned even in Tier 3. |

### Nits (8)

| ID | Domain | Finding |
|----|--------|---------|
| N1 | Rust | `TelemetryConfig` struct wraps a single `bool` — could be simplified to a direct field. |
| N2 | Rust | `AnonymizedTelemetrySummary` duplicates `VerdictSummary` subset — could derive via `From` impl. |
| N3 | npm | Missing explanatory comment on `BUILTIN_MODULES` set construction logic. |
| N4 | npm | `runCli` default parameter `argv = process.argv.slice(2)` in exported library API — surprising for library consumers. |
| N5 | CI/CD | `Publish summary` step in `release-npm-wrapper.yml` missing `set -euo pipefail`. |
| N6 | CI/CD | `perf_budget.sh` env defaults are hardcoded in both script and workflow YAML — could use repo variables. |
| N7 | Tests | `perf_budget.rs` missing module-level doc comment (inconsistent with other test files). |
| N8 | Docs | `support-matrix-v1.md` and `DOGFOOD_RELEASE_CHANNEL.md` redundantly define channel semantics. |

---

## Missing Test Coverage

The following new code paths lack test coverage:

| Feature | Gap |
|---------|-----|
| npm wrapper package | **No tests at all** for 435-line `generate-resolution-snapshot.js` — classification, path normalization, arg parsing, builtin detection all untested |
| `stale_baseline: warn` policy | No test asserting warn doesn't block the gate when stale entries exist |
| `baseline --refresh` metadata | No test verifying `generated_from` provenance is preserved (or correctly updated) |
| `handle_check_with_diff` + stale policy | Interaction between diff mode and stale baseline policy is untested |
| Telemetry schema completeness | Only `event` field asserted; `schema_version`, `project_fingerprint`, `summary` not validated |
| Structured snapshot multi-edge | Round-trip test uses single-edge fixture only |
| `--structured-snapshot-out` with absolute path | Only relative path tested |

---

## What's Done Well

- **Deterministic baseline refresh** — `refresh_baseline` produces identical output regardless of input ordering; well-tested with the determinism assertion.
- **Clean extension pattern** — `VerdictBuildOptions` + `build_verdict_with_options` extends the verdict builder without breaking existing callers.
- **Three-way telemetry priority** — flag > counter-flag > config resolution is explicit and readable.
- **Channel gating for legacy parser** — Beta-only gate on raw tsc trace parsing with clear error message. Good security boundary.
- **CI workflow validation** — `release-npm-wrapper.yml` includes version-match validation, dist-tag verification post-publish, and provenance support.
- **Perf budget test** — Configurable via env vars with sensible defaults; timer placement is correct (measures only `run()`, not fixture setup).
- **Clap conflict declarations** — `conflicts_with_all` on `--structured-snapshot-in` prevents ambiguous input combinations at the CLI layer.

---

## Recommended Action Items (Priority Order)

1. **Fix `spawnSync` signal handling** in `npm/specgate/bin/specgate.js` — reflect signal kills in exit code
2. **Fix `refresh_baseline` metadata** — update `generated_from` with current tool version/git sha, not stale values
3. **Strip build metadata from version** in `release-npm-wrapper.yml` — `version="${version%%+*}"`
4. **Add path traversal validation** for `package_dir` in `release-npm-wrapper.yml`
5. **Add npm wrapper tests** — at minimum cover `classifyResolution`, `toProjectPath`, `parseArgs`, and a happy-path integration
6. **Document `--tsc-command`/`--allow-shell`** in `spec-language.md`
7. **Fix module resolution cache key** for case-insensitive filesystems
8. **Remove `Value::clone()`** in structured trace parsing — pass ownership instead
9. **Add retry/backoff to npm publish verification** or document the known flakiness
10. **Add missing test coverage** per the gaps table above

---

*Generated by 5 parallel code-reviewer agents, synthesized by orchestrator.*
