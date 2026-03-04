# Changelog

All notable changes to Specgate are documented in this file.

## [Unreleased]

- No unreleased entries yet.

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
