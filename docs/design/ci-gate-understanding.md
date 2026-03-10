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
- **Informational checks:** optional metrics-mode runs, `doctor compare` explorations, and additional non-gate fixture collections.
- **Policy governance:** enforce exactly one PR path - `specgate policy-diff` (recommended for explicit governance artifacts) or `check --since <base-ref> --deny-widenings` (single-command enforcement).

`golden_corpus` (`tests/golden_corpus.rs`) remains informational and is intentionally
outside the merge gate as future-proxy coverage for deferred rule families.
`golden_corpus_gate` remains part of `./scripts/ci/mvp_gate.sh` as the merge-gate contract check.

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
- Sorted violations for consistent diff
- Stable behavior with explicit `--output-mode metrics` when telemetry is collected

**Use in CI:** Use deterministic mode for merge gates.

### Metrics Mode

```bash
specgate check --output-mode metrics
```

**Adds:**
- `metrics.timings_ms` (phase timings)
- `metrics.total_ms` (total duration)
- Performance counters

**Use in CI:** Acceptable when collecting telemetry. Exit code behavior is unchanged.

## Policy Governance in CI

### Choose one enforcement path

Use one of the following as the blocking governance check for PR CI.

```bash
specgate policy-diff --base origin/main --format json
```

```bash
specgate check --since origin/main --deny-widenings
```

- `policy-diff` is the recommended default when you want explicit diff output for `.spec.yml` policy changes.
- `check --deny-widenings` reuses the same policy diff engine and fails with code `1` when widening is detected.
- `check --deny-widenings` requires `--since <base-ref>` and returns code `2` on governance runtime/parse failures.

## SARIF in CI

```bash
specgate check --since origin/main --format sarif > specgate.sarif
```

```yaml
- name: Upload SARIF
  uses: github/codeql-action/upload-sarif@v3
  with:
    sarif_file: specgate.sarif
```

---

## Baseline Fingerprinting

## Baseline policy reference

Stale baseline entries are warn-by-default, opt-in fail via `stale_baseline: fail`, and never auto-pruned; see [BASELINE_POLICY.md](BASELINE_POLICY.md) for canonical policy and runbook.

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
      specgate check --output-mode deterministic | tee .specgate-verdict.json
    else
      specgate check --since origin/main --output-mode deterministic | tee .specgate-verdict.json
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
  "verdict_schema": "1.0",
  "schema_version": "2.2",
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

- `verdict_schema`: Version of the verdict output schema (currently `1.0`)
- `schema_version`: Version of the spec language used for evaluation (currently `2.2`)
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
    name: Specgate Gate
    runs-on: ubuntu-latest
    timeout-minutes: 15

    permissions:
      contents: read
      security-events: write

    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0  # Required for --since / --base governance checks

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@v1
        with:
          toolchain: 1.88.0
      
      - name: Install Specgate
        run: |
          cargo install --locked --git https://github.com/treygoff24/specgate --tag v0.1.0-rc3 --root "$RUNNER_TEMP/specgate" --force
          echo "$RUNNER_TEMP/specgate/bin" >> "$GITHUB_PATH"
          # Canonical release-binary + checksum flow:
          # ../examples/specgate-consumer-github-actions.yml

      - name: Run Specgate check
        run: |
          if [ "${{ github.event_name }}" == "pull_request" ]; then
            specgate check --since origin/main --output-mode deterministic | tee .specgate-verdict.json
          else
            specgate check --output-mode deterministic | tee .specgate-verdict.json
          fi

      - name: Detect policy widenings
        if: github.event_name == 'pull_request'
        run: |
          specgate policy-diff --base origin/main --format json | tee .specgate-policy-diff.json

      - name: Render SARIF
        if: always()
        run: |
          if [ "${{ github.event_name }}" == "pull_request" ]; then
            specgate check --since origin/main --format sarif > specgate.sarif
          else
            specgate check --format sarif > specgate.sarif
          fi

      - name: Upload SARIF
        if: always() && hashFiles('specgate.sarif') != ''
        uses: github/codeql-action/upload-sarif@v3
        with:
          sarif_file: specgate.sarif
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
- If using `--output-mode metrics`, verify timing fields are optional and not used as pass/fail criteria
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
