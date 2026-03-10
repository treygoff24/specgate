# Historical Wave 0: Contract Lock

> **Historical note:** This is the original Wave 0 lock snapshot. It is kept
> for historical context and may not reflect later shipped behavior. Use
> `docs/reference/spec-language.md`, `docs/reference/operator-guide.md`, and
> `docs/roadmap.md` for current truth.

This document captures the contract lock established in Wave 0, ensuring stability for contract-critical surfaces.

## Version Contract

### Supported Versions

- **Versions**: `2.2`, `2.3` (exact match against supported versions)
- **Constants**: `SUPPORTED_SPEC_VERSIONS` and `CURRENT_SPEC_VERSION` in `src/spec/types.rs`

### Compatibility Policy

Specgate enforces **strict version matching**:

1. `version: "2.2"` and `version: "2.3"` are accepted in spec files
2. `version: "2.3"` is also accepted and is required when using `boundaries.contracts`
3. Versions `2` or `2.0` are rejected with a clear error message
4. Version validation occurs during `specgate validate` and `specgate check`

### Rationale

Strict matching ensures:
- Explicit version updates when specs change
- Unambiguous version compatibility
- Foundation for future multi-version support

## CLI Semantics

### Baseline Diff Mode

| Flag | Status | Description |
|------|--------|-------------|
| `--baseline-diff` | ✅ Preferred | Output diff between current and baseline violations |
| `--baseline-new-only` | ✅ Preferred | Show only new violations in diff format |
| `--diff` | ⚠️ Deprecated | Alias for `--baseline-diff` (emits warning) |
| `--diff-new-only` | ⚠️ Deprecated | Alias for `--baseline-new-only` (emits warning) |

### Git Blast-Radius Mode

| Flag | Status | Description |
|------|--------|-------------|
| `--since <git-ref>` | ✅ New | Only check modules affected since git reference |

**Blast-radius computation**:
1. Run `git diff --name-only --diff-filter=ACMRT <ref>`
2. Map changed files to their modules
3. Compute transitive importers of affected modules
4. Filter violations to blast radius

## Boundary Rules Contract

### `allow_imports_from`

- **Type**: `Vec<String>` (module IDs)
- **Semantics**: Exact module ID matching (case-sensitive)
- **Default**: Empty (all imports allowed)
- **Non-empty**: Only listed modules can be imported

### `public_api`

- **Type**: `Vec<String>` (glob patterns)
- **Semantics**: Glob patterns matched against normalized file paths
- **Default**: Empty (all files public)
- **Non-empty**: Only matching files are public; internal file imports trigger violation

## Test Coverage

Contract tests are in `tests/contract_fixtures.rs`:

1. **Allowlist behavior**: Exact module ID matching, empty vs non-empty semantics
2. **Public API behavior**: Glob matching, internal file violations
3. **Blast-radius behavior**: Transitive importers, cycle handling
4. **Deprecated flags**: Warning emission
5. **Version enforcement**: Reject 2, accept 2.2 and 2.3

## Changelog

### Wave 0 Changes

- Added `--baseline-diff` and `--baseline-new-only` flags (preferred)
- Deprecated `--diff` and `--diff-new-only` with warnings
- Added `--since <git-ref>` for blast-radius mode
- Locked supported versions to exact-match semantics, later extended to `2.2` and `2.3`
- Added `src/git_blast/` module for git integration
- Added contract fixtures in `tests/contract_fixtures.rs`
- Updated `docs/spec-language.md` with contract documentation

### Post-Wave TS/JS v1 Additions (Channel-Gated)

- Added doctor compare parser modes: `auto|structured|legacy`.
- Added structured snapshot IO flags:
  - `--structured-snapshot-in <path>`
  - `--structured-snapshot-out <path>`
- Added baseline refresh command surface:
  - `specgate baseline --refresh`
- Added config keys:
  - `stale_baseline: warn|fail`
  - `release_channel: stable|beta`
  - `telemetry.enabled: bool` (opt-in; default off)
