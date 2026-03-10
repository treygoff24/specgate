# Specgate Spec Language

## Version Contract

Specgate enforces strict version compatibility for spec files.

### Supported Versions

- **Current version**: `2.3`
- **Backward compatibility**: `2.2` remains supported
- **Policy**: Exact version match against supported versions

### Version Compatibility

| Version | Status | Notes |
|---------|--------|-------|
| 2.3 | ✅ Supported | Current version; required for `boundaries.contracts` |
| 2.2 | ✅ Supported | Backward-compatible legacy version (no `boundaries.contracts`) |
| 2.0 | ❌ Not supported | Must upgrade to 2.2 or 2.3 |
| 2 | ❌ Not supported | Must upgrade to 2.2 or 2.3 |

### Why Strict Matching?

The spec language is evolving rapidly during foundation phases. We enforce exact version matching to:

1. Force explicit version updates when specs change
2. Make version compatibility unambiguous
3. Enable future support for multiple versions if needed

### Migration

#### Legacy `2` → supported versions

Change:

```yaml
version: "2"
```

To either supported version:

```yaml
version: "2.2"
```

or:

```yaml
version: "2.3"
```

#### `2.2` → `2.3` upgrade path

If you need contract boundaries, upgrade spec files from:

```yaml
version: "2.2"
```

to:

```yaml
version: "2.3"
```

`boundaries.contracts` is only valid in version `2.3`.

## CLI Semantics (Wave 0 Lock)

### Baseline Diff Mode

Compare current violations against baseline:

```bash
# Show all violations with diff formatting (preferred)
specgate check --baseline-diff

# Show only new violations (preferred)
specgate check --baseline-diff --baseline-new-only
```

#### Deprecated Flags

The following flags are deprecated and will be removed in a future release:

| Deprecated | Use Instead |
|------------|-------------|
| `--diff` | `--baseline-diff` |
| `--diff-new-only` | `--baseline-new-only` |

Using deprecated flags will emit a warning to stderr.

### Git Blast-Radius Mode

Only check modules affected by changes since a git reference:

```bash
# Check only modules changed since last commit
specgate check --since HEAD~1

# Check only modules changed since branching from main
specgate check --since main
```

The blast radius includes:
1. Files directly changed since the reference
2. Modules containing changed files
3. Modules that transitively import from affected modules

### Baseline Hygiene + Telemetry

```bash
# Regenerate baseline from current violations
specgate baseline --output .specgate-baseline.json

# Refresh and prune stale entries
specgate baseline --refresh --output .specgate-baseline.json

# Opt in to telemetry for one run
specgate check --telemetry
```

Config keys in `specgate.config.yml`:

```yaml
stale_baseline: warn   # or fail
release_channel: stable # or beta
telemetry:
  enabled: false
```

- `stale_baseline` follows canonical baseline policy (warn-by-default, opt-in fail via `stale_baseline: fail`, no auto-prune); see [baseline-policy.md](../design/baseline-policy.md).
- `release_channel: beta` enables beta-only doctor compare legacy trace fallback.
- telemetry is opt-in by default and can be toggled per run with `--telemetry` / `--no-telemetry`.
- `strict_ownership: true` makes `specgate doctor ownership` exit nonzero when it finds unclaimed files, overlaps, orphaned specs, duplicate module ids, or invalid ownership globs.

### Doctor Ownership

```bash
# Human-readable ownership report
specgate doctor ownership --project-root .

# Structured ownership report
specgate doctor ownership --project-root . --format json
```

`doctor ownership` reports:
- `unclaimed_files`: tracked source files not matched by any spec ownership glob
- `overlapping_files`: tracked source files claimed by multiple specs
- `orphaned_specs`: specs whose `boundaries.path` matches zero tracked source files
- `duplicate_module_ids`: duplicate module declarations across spec files
- `invalid_globs`: ownership globs that failed to compile

By default this command is diagnostic-only and exits `0`. With `strict_ownership: true`, findings promote the command to a nonzero exit for CI gating.

### Doctor Compare Parser Modes

```bash
# Structured snapshots only
specgate doctor compare --parser-mode structured --structured-snapshot-in trace.json

# Auto: structured first, then beta-only legacy fallback
specgate doctor compare --parser-mode auto --tsc-trace trace.log
```

Modes:
- `structured`: requires structured JSON snapshot payload.
- `auto`: prefers structured parsing and falls back to raw trace text only in `beta` channel.
- `legacy`: raw `tsc --traceResolution` text only (beta channel only).

### Shell Command Execution

```bash
# Run a command that emits structured trace JSON to stdout
specgate doctor compare --tsc-command "npx tsc --traceResolution" --allow-shell

# Write normalized trace output to a file
specgate doctor compare --tsc-trace trace.log --structured-snapshot-out snapshots/normalized.json
```

#### `--tsc-command` and `--allow-shell`

| Flag | Description |
|------|-------------|
| `--tsc-command <cmd>` | Command that emits compatible JSON to stdout |
| `--allow-shell` | Explicit opt-in for running `--tsc-command` through the system shell |

**⚠️ SECURITY WARNING:** `--tsc-command` executes the provided command via `sh -lc`, which can run arbitrary shell code. You must also pass `--allow-shell` to opt into execution. Only use this flag with trusted commands.

#### `--structured-snapshot-out`

| Flag | Description |
|------|-------------|
| `--structured-snapshot-out <path>` | Write normalized structured trace snapshot JSON to this path |

This writes the normalized trace data to a file for caching or offline comparison.

## Boundary Rules

### `allow_imports_from`

Defines which modules this module is allowed to import from.

```yaml
boundaries:
  allow_imports_from:
    - core/api
    - shared/utils
```

**Contract**:
- Field omitted: All imports allowed (no restriction)
- Empty list: No cross-module imports allowed
- Non-empty list: Only listed modules can be imported
- Exact module ID matching (case-sensitive)

### `public_api`

Defines which files are part of the public API.

```yaml
boundaries:
  public_api:
    - src/core/index.ts
    - src/core/public/**/*
```

**Contract**:
- Glob patterns matched against normalized file paths
- Files NOT matching `public_api` are considered internal
- Importing from internal files triggers `boundary.public_api` violation

### `contracts` (version `2.3`)

`boundaries.contracts` is available only in spec version `2.3`.
For the full schema and enforcement semantics, see
[boundary-contracts-v2](../design/boundary-contracts-v2.md).

Each contract can also set `envelope` to declare validation-site requirements:

```yaml
boundaries:
  contracts:
    - id: create_user
      contract: contracts/create-user.json
      envelope: required | optional  # default: optional
      match:
        files: ["src/api/handlers/user.ts"]
```

- `envelope: optional` (default): Specgate records matches but does **not** enforce envelope validation.
- `envelope: required`: Specgate performs a targeted AST check on all files that match `match.files`.
  - The check passes only if the file imports the envelope package at runtime (type-only imports do **not** count).
  - The check passes only if the file calls `boundary.validate('contract_id', data)` with the exact `id` for that contract.
  - If `match.pattern` is set, the AST check is scoped to that one exported function's subtree.

When `envelope: required`, missing checks are reported as **warnings**, not errors.
The `envelope` configuration (package/function matching) is set in `specgate.config.yml` under the `envelope` section.
