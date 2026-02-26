# CI Gate Understanding

**How Specgate works as a reliable CI merge gate.**

This document explains the CI patterns, exit codes, and deterministic contracts that make Specgate reliable in automated pipelines.

---

## The Problem Specgate Solves

Traditional architecture tests suffer from:
- **Flaky output** — Timestamps, paths, ordering vary between runs
- **Noise** — Existing violations hide new problems
- **Blast radius** — One file change shouldn't require full re-check

Specgate solves these with:
- **Deterministic output** — Byte-identical for same inputs
- **Baseline fingerprinting** — Suppress known violations
- **Git-aware blast-radius** — Only check affected modules

## Gate taxonomy

- **Gating checks (must pass to merge):** `./scripts/ci/mvp_gate.sh` as defined in `mvp-merge-gate.md` (contract fixtures, golden corpus, tier-A gate, baseline behavior).
- **Informational checks:** local metrics-mode runs, optional `doctor compare` explorations, and additional non-gate fixture collections.

---

## Exit Codes

| Code | Meaning | Action |
|------|---------|--------|
| `0` | Pass | Build continues |
| `1` | Policy violation | Build fails (fix the code) |
| `2` | Runtime error | Build fails (investigate config) |

### Example Handling

```bash
specgate check
exit_code=$?

if [ $exit_code -eq 0 ]; then
  echo "✅ All checks passed"
elif [ $exit_code -eq 1 ]; then
  echo "❌ Policy violations found"
  exit 1
else
  echo "⚠️ Runtime error - check config"
  exit 2
fi
```

---

## Output Modes

### Deterministic Mode (Default)

```bash
specgate check --output-mode deterministic
```

**Guarantees:**
- Byte-identical output for same inputs
- Excludes runtime `metrics` section and timing fields
- Sorted violations for consistent diff

**Use in CI:** Always use deterministic mode for merge gates.

### Metrics Mode

```bash
specgate check --output-mode metrics
```

**Adds:**
- `metrics.timings_ms` (phase timings)
- `metrics.total_ms` (total duration)
- Performance counters

**Use locally:** For debugging performance, not for CI gating.

---

## Baseline Fingerprinting

### What It Does

Baselines suppress known violations while still reporting them. New violations still fail the build.

### Creating a Baseline

```bash
specgate baseline --output .specgate-baseline.json
```

Baseline file format:

```json
{
  "version": "1",
  "generated_from": {
    "tool_version": "0.1.0",
    "git_sha": "abc123",
    "config_hash": "...",
    "spec_hash": "..."
  },
  "entries": [
    {
      "fingerprint": "sha256:...",
      "positional_fingerprint": "sha256:...",
      "rule": "boundary.allow_imports_from",
      "severity": "error",
      "message": "...",
      "from_file": "src/api/handlers/user.ts",
      "to_file": "src/infra/db/index.ts",
      "from_module": "core/api",
      "to_module": "infra/db",
      "line": 15,
      "column": 8
    }
  ]
}
```

### Fingerprint Calculation

Hash of normalized tuple:
```
module|rule|severity|file|line|import_source|resolved_target
```

- Path separators normalized
- Repo-root-relative paths
- Stable across machines

### CI Pattern with Baseline

```yaml
# Load baseline at start
- name: Load Baseline
  run: |
    git checkout origin/main -- .specgate-baseline.json 2>/dev/null || true

# Check with baseline
- name: Specgate Check
  run: specgate check

# Update baseline after merge
- name: Update Baseline
  if: github.ref == 'refs/heads/main'
  run: |
    specgate baseline --output .specgate-baseline.json
    git config user.name "CI"
    git config user.email "ci@example.com"
    git add .specgate-baseline.json
    git commit -m "chore: update specgate baseline" || true
    git push
```

### Verdict Fields

```json
{
  "status": "fail",
  "summary": {
    "total_violations": 6,
    "new_violations": 1,
    "baseline_violations": 5,
    "suppressed_violations": 0,
    "error_violations": 1,
    "warning_violations": 5,
    "new_error_violations": 1,
    "new_warning_violations": 0,
    "stale_baseline_entries": 0
  }
}
```

- `summary.baseline_violations`: Violations matching fingerprints
- `summary.new_violations`: Violations not in baseline
- `summary.new_error_violations`: New error violations (drives exit `1`)
- `summary.stale_baseline_entries`: Baseline entries with no current match (stability signal)

---

## Git Blast-Radius Mode

### What It Does

Only check modules affected by changes since a git reference. Reduces CI time and focuses on actual impact.

### Usage

```bash
# Check only modules changed since last commit
specgate check --since HEAD~1

# Check only modules changed since branching from main
specgate check --since origin/main

# In PR context
specgate check --since ${{ github.base_ref }}
```

### Blast Radius Computation

1. Run `git diff --name-only --diff-filter=ACMRT <ref>`
2. Map changed files and changed spec files to their modules
3. Compute transitive importers of affected modules
4. Filter violations to blast radius

### Requirements

- **Full git history** — `fetch-depth: 0` in checkout action
- **Valid git ref** — Must exist in repository

```yaml
- uses: actions/checkout@v4
  with:
    fetch-depth: 0  # Required for --since
```

---

## Tier A Gate Contract

### What Makes Tier A CI-Safe

From [Tier A Fixture Design](tier-a-fixture-design-v1.md):

1. **Intro fails now** — On implemented rule
2. **Fix passes now** — Same config
3. **Exact expected violations** — Not "contains" matching
4. **Single-intent** — One target rule
5. **Deterministic ordering** — Verified across 3 runs

### Running the Tier A Gate

```bash
# Run Tier A gate tests
cargo test tier_a_golden

# Exit 0 = gate passes
# Exit non-zero = gate fails (do not merge)
```

### CI Integration

```yaml
- name: Tier A Gate
  run: cargo test tier_a_golden
```

---

## Diff Mode

### Showing Changes

```bash
# Show all violations with diff formatting
specgate check --baseline-diff

# Show only new violations
specgate check --baseline-diff --baseline-new-only
```

### Deprecated Flags

| Deprecated | Use Instead |
|------------|-------------|
| `--diff` | `--baseline-diff` |
| `--diff-new-only` | `--baseline-new-only` |

Deprecated flags emit warnings to stderr.

---

## Complete CI Example

```yaml
name: Specgate CI

on:
  push:
    branches: [main]
  pull_request:

jobs:
  specgate:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0  # Required for blast-radius
      
      - name: Setup Rust
        uses: dtolnay/rust-toolchain@stable
      
      - name: Build Specgate
        run: cargo build --release
      
      - name: Tier A Gate
        run: cargo test tier_a_golden
      
      - name: Load Baseline
        run: |
          if [ "${{ github.event_name }}" == "pull_request" ]; then
            git checkout origin/main -- .specgate-baseline.json 2>/dev/null || true
          fi
      
      - name: Specgate Check
        run: |
          if [ "${{ github.event_name }}" == "pull_request" ]; then
            ./target/release/specgate check --since origin/main --output-mode deterministic
          else
            ./target/release/specgate check --output-mode deterministic
          fi
      
      - name: Contract Tests
        run: cargo test contract_fixtures
      
      - name: Update Baseline
        if: github.ref == 'refs/heads/main' && success()
        run: |
          ./target/release/specgate baseline --output .specgate-baseline.json
          git config user.name "CI Bot"
          git config user.email "ci@example.com"
          git add .specgate-baseline.json
          git commit -m "chore: update specgate baseline [skip ci]" || echo "No baseline changes"
```

---

## Troubleshooting

### "Exit code 2" (Runtime Error)

Check:
- Config file syntax (`specgate validate`)
- Spec file versions (must be `"2.2"`)
- Module IDs are unique
- File paths exist

### "Flaky CI"

Ensure:
- Using `--output-mode deterministic`
- Not using `--output-mode metrics` in CI
- Baseline file is committed
- Git history is available for `--since`

### "Baseline not suppressing"

Verify:
- Baseline file exists and is valid JSON
- Fingerprints match (same config/spec hashes)
- No structural changes to violations (file renames, etc.)

---

## Best Practices

1. **Always use deterministic mode in CI** — No exceptions
2. **Commit baselines to main branch** — Auto-update after merge
3. **Use blast-radius on PRs** — Faster feedback
4. **Run full check on main** — Catch edge cases
5. **Gate on Tier A** — Deterministic contract enforcement
6. **Monitor summary.stale_baseline_entries** — indicates churned/unused baseline debt
7. **Review stale entries** — Periodically clean baselines

---

## See Also

- [Operator Guide](OPERATOR_GUIDE.md) — Full onboarding
- [MVP Merge Gate](mvp-merge-gate.md) — Single merge-ready checklist
- [Wave 0 Contract](../WAVE0_CONTRACT.md) — Locked semantics
- [Tier A Fixture Design](tier-a-fixture-design-v1.md) — Gate specification
- [Implementation Plan](specgate-implementation-plan-v1.1.md) — MVP status
