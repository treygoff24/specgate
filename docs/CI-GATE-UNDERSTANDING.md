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

- **Gating checks (must pass to merge):** `./scripts/ci/mvp_gate.sh` as defined in `mvp-merge-gate.md` (contract fixtures, golden_corpus_gate, tier-A gate, baseline behavior).
- **Informational checks:** local metrics-mode runs, optional `doctor compare` explorations, and additional non-gate fixture collections.

`golden_corpus` (`tests/golden_corpus.rs`) remains informational and is intentionally
outside the merge gate as future-proxy coverage for deferred rule families.

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

## Baseline policy reference

- See [BASELINE_POLICY.md](BASELINE_POLICY.md) for canonical stale-entry and refresh behavior.
- In this release stage, stale baseline hits are tracked for triage and do not hard-fail the merge gate.

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
    if [ -f .specgate-baseline.json ]; then
      echo "Using tracked baseline file"
    fi

# Check with baseline
- name: Specgate Check
  run: |
    if [ "${{ github.ref }}" == "refs/heads/main" ]; then
      specgate check --output-mode metrics | tee .specgate-verdict.json
    else
      specgate check --since origin/main --output-mode metrics | tee .specgate-verdict.json
    fi

# Baseline maintenance is manual
- name: Update Baseline
  if: github.ref == 'refs/heads/main'
  run: |
    echo "Baseline refresh is handled through approved maintenance PRs"
    echo "Use: specgate baseline --output .specgate-baseline.json"
    echo "Then commit through normal review flow."
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
        uses: dtolnay/rust-toolchain@1.88.0
      
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
          echo "Manual baseline maintenance should be handled in dedicated PR windows."
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

1. **Use deterministic or metrics mode in CI** — metrics mode is acceptable when collecting telemetry and keeps gate exit-code behavior unchanged
2. **Do not auto-update baselines from CI** — Refresh baselines only in approved maintenance windows and commit through normal PRs
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
- [Baseline policy](BASELINE_POLICY.md) — Baseline ownership and stale-entry guidance
- [Releasing guide](../RELEASING.md) — Release mechanics
- [Dogfood rollout checklist](DOGFOOD_ROLLOUT_CHECKLIST.md) — Pilot readiness
