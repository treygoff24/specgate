# Specgate Spec Language

## Version Contract

Specgate enforces strict version compatibility for spec files.

### Supported Version

- **Current version**: `2.2`
- **Policy**: Exact match required

### Version Compatibility

| Version | Status | Notes |
|---------|--------|-------|
| 2.2 | ✅ Supported | Current version |
| 2.0 | ❌ Not supported | Must upgrade to 2.2 |
| 2 | ❌ Not supported | Must upgrade to 2.2 |

### Why Strict Matching?

The spec language is evolving rapidly during foundation phases. We enforce exact version matching to:

1. Force explicit version updates when specs change
2. Make version compatibility unambiguous
3. Enable future support for multiple versions if needed

### Migration

To update your spec files, change:

```yaml
version: "2"
```

to:

```yaml
version: "2.2"
```

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

- `stale_baseline: fail` makes stale baseline entries policy-failing.
- `release_channel: beta` enables beta-only doctor compare legacy trace fallback.
- telemetry is opt-in by default and can be toggled per run with `--telemetry` / `--no-telemetry`.

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
