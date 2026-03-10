# Changelog

All notable changes to Specgate are documented in this file.

## [Unreleased]

### Added
- **Envelope AST static check (Phase 5):** When a contract declares `envelope: required`, specgate performs a targeted AST analysis on matched source files to verify they import the envelope package and call the validation function with the correct contract ID. Violations are warnings. Configurable via `envelope` section in `specgate.config.yml`.
- `specgate policy-diff` for comparing `.spec.yml` policy between git refs with `human`, `json`, and `ndjson` output, exit codes `0/1/2`, shallow clone diagnostics with `fetch-depth: 0` guidance, and fail closed widening treatment for policy deletions plus rename or copy operations in the MVP
- `match.pattern` function scoping: envelope checks can be scoped to a specific exported function
- `EnvelopeConfig` in `specgate.config.yml`: `enabled`, `import_patterns`, `function_pattern`
- New module `src/rules/envelope.rs` for targeted AST analysis
- Adversarial test fixture suite with 14 scenarios covering boundary evasion, re-export chains, and edge cases
- `--format sarif` flag for SARIF 2.1.0 output (GitHub Code Scanning integration)
- `specgate doctor ownership` for ownership diagnostics with human/json output, strict CI gating, and reporting for unclaimed files, overlaps, orphaned specs, duplicate module ids, and invalid globs

### Changed
- `ContractRuleViolation` now carries its own `severity` instead of being hardcoded to Error
- `check_match_patterns()` returns resolved file paths for reuse by envelope checker
- `policy-diff` exit-2 output is now explicitly non-authoritative: on runtime or parse failures, classification output is suppressed (empty `diffs`, zeroed summary counters), while structured `errors` remain present; `ndjson` adds `type: "error"` events before summary.
- Operator note: config governance diffing for `specgate.config.yml` is deferred-by-decision for this release; `policy-diff` remains scoped to `.spec.yml` snapshots.

### npm Wrapper Hardening (P3.2)

- **Version introspection** — `wrapperVersion` export and `wrapper_version` field in snapshot output for debugging and parity tracking.
- **Diagnostic error messages** — binary-not-found errors now show platform, arch, and searched candidate paths.
- **Signal handling hardening** — extracted `signalExitCode()` helper, platform-safe signal mapping, removed dead code paths.
- **Support matrix validation** — contract tests verifying release target parity between Rust cross-compile and npm wrapper platform mapping.
- **CI smoke test** — new workflow runs npm wrapper tests on Node 18/20/22 across Ubuntu and macOS for every PR.
- **End-to-end smoke test** — merge gate builds the Rust binary and runs `specgate check` on the openclaw-scale fixture via the npm wrapper, verifying the full binary-forwarding path in a fresh environment.

### OpenClaw-Scale Regression Gate (P3.3)

- **Expanded fixture** — 3-package monorepo (`web`, `alpha`, `@openclaw/shared`) with tsconfig extends chains, star re-exports, re-export chains, cross-package imports, type-only imports, dynamic imports, circular dependencies, and intentional boundary violations.
- **Regression test suite** — expanded from 3 to 9 tests covering init discovery, cross-package resolution, boundary violations, circular dependency handling, re-export edge counts, and workspace packages in verdict.
- **Performance budget** — openclaw-scale fixture perf test with configurable budget (default 5s, override via `SPECGATE_OPENCLAW_PERF_BUDGET_MS`).
- **Module map construction budget** — isolated perf test for `ModuleResolver` initialization (glob matching + file-to-module mapping), separate from full check pipeline (default 2s, override via `SPECGATE_MODULE_MAP_BUDGET_MS`).
- **CI gate coverage** — merge gate runs the full TS/JS parity suite (parser, resolver, rules) on every PR unconditionally — stricter than path-filtered triggering.

## [0.3.0] - 2026-03-04

### Full Monorepo Support (P3.1)

- **Nearest-tsconfig multi-context resolver** — files in monorepo packages with their own `tsconfig.json` now automatically resolve path aliases using their owning tsconfig, not the root.
- **`tsconfig_filename` config** — new `specgate.config.yml` field to override the tsconfig filename (default: `"tsconfig.json"`), for repos using `tsconfig.base.json` or `tsconfig.build.json`.
- **Workspace package discovery in check** — `specgate check` detects workspace packages (pnpm/npm) and includes them in verdict JSON (`workspace_packages` field).
- **Doctor workspace summary** — `specgate doctor` prints discovered workspace packages with their tsconfig paths, and surfaces `tsconfig_filename` overrides.
- **npm snapshot batch mode** — `specgate-resolution-snapshot --workspace` generates resolution snapshots for all workspace packages in one invocation. Supports `--tsconfig-filename` for custom tsconfig names.
- **Monorepo integration test suite** — new fixture and 6 tests validating cross-package alias resolution, boundary enforcement, workspace package discovery in verdicts, and deterministic output.
- **Perf benchmarks** — tsconfig cache efficiency tests and single-root regression benchmark to guard against performance regressions.

## [0.2.0] - 2026-03-03

### MVP Foundation

- Introduced spec-language-driven architecture policy checks for TypeScript projects.
- Added spec validation with deterministic diagnostics and explicit exit-code behavior.
- Implemented dependency rules (allow/forbid semantics, test carve-outs, deterministic ordering).
- Added baseline fingerprinting with new-vs-baseline classification and stale baseline accounting.
- Added git-aware blast-radius (`--since`) analysis to scope evaluation to impacted modules/importers.
- Locked deterministic verdict output suitable for CI merge gates.
- Added CI gate script and test suites for merge enforcement (`scripts/ci/mvp_gate.sh`, tier-A + gate fixtures).

### Boundary Contracts V2

- Added contracts model in spec language (`contracts[]`, `imports_contract`, contract refs, file matching).
- Added contract validation (duplicate IDs, bad refs, unresolved matches, extension checks, version compatibility).
- Added contract enforcement in rule evaluation with deterministic violations.
- Added structured diagnostics fields (`expected`, `actual`, `remediation_hint`, `contract_id`) across verdict violations.
- Added multi-format outputs (`human`, `json`, `ndjson`) and TTY-aware default format behavior.
- Decoupled verdict schema versioning from spec schema version with `verdict_schema` in verdict output.

### TS/JS Ecosystem

- Added nearest-tsconfig context per file for accurate resolver configuration.
- Added NodeNext extension alias resolution (.js → .ts/.tsx).
- Expanded resolver condition names (node, types) for broader package compatibility.
- Added workspace package discovery (pnpm + npm workspaces).
- Added import attributes parser parity with OXC.
- Added type-only import policy toggle (`enforce_type_only_imports` config).
- Validated barrel/re-export robustness across resolver paths.
- Added OpenClaw-scale regression test suite for real-world project coverage.
- Added doctor compare structured snapshot round-trip with mismatch categories.
- Hardened npm wrapper (signal handling, case-normalized cache, peer dependency checks, path traversal validation).
- Added comprehensive npm wrapper test suite (53 tests).

### Code Quality

- Refactored `AnonymizedTelemetrySummary` to eliminate field duplication.
- Extracted doctor command handlers into separate module (`src/cli/doctor.rs`).
- Isolated telemetry tests for independence from shared state.
- Consolidated perf budget defaults to single source of truth.
- Added comprehensive test coverage for diff+stale, telemetry schema, and snapshot round-trips.

### Dogfood/Operations

- Added `specgate doctor compare` for resolver parity/debug comparison workflows.
- Added opt-in anonymized telemetry and stable event schema for check-completed signals.
- Added release-channel and rollout operations docs (dogfood checklist, success metrics, release channel guidance).
- Hardened release asset verification with checksum and binary smoke-check workflows.

## [0.1.0] - 2026-02-26

### Added

- Initial MVP release foundation and CI-gating flow.
