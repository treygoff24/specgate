# baseline/

Wave 2C baseline support.

## Responsibilities

- Generate stable fingerprints for policy violations
- Read/write deterministic baseline JSON (`.specgate-baseline.json` by default)
- Classify violations as `new` vs `baseline` for report-only behavior
- Track stale baseline entries for hygiene visibility

## Baseline Lifecycle

### Initial Creation

```bash
specgate baseline --write .specgate-baseline.json
```

This creates a baseline file containing fingerprints for all current violations.

### CI Integration

1. Check with baseline classification:
   ```bash
   specgate check
   ```

2. Violations are classified as:
   - **New**: Not in baseline → enforced by severity
   - **Baseline**: Fingerprint matches → report-only (does not fail CI)

### Stale Entry Hygiene

The check output includes `stale_baseline_entries` in the summary when baseline
entries no longer match any current violations. This indicates:

- A violation was fixed but the baseline wasn't updated
- A rule was removed or changed scope
- Files were renamed/moved affecting fingerprint paths

To clean up stale entries:

```bash
specgate baseline --write .specgate-baseline.json
git add .specgate-baseline.json
git commit -m "chore: update baseline"
```

### Fingerprint Stability

Fingerprints are stable across runs when:
- File paths (repo-relative) are unchanged
- Rule IDs, severity, line numbers stay the same
- Import sources and resolved targets remain consistent

Fingerprints may change when:
- Files are renamed or moved
- Rules are refactored (new rule IDs)
- Violation details (line numbers, messages) change

### Governance Fields

The verdict output includes these baseline-related fields:

- `summary.stale_baseline_entries`: Count of baseline entries with no current match
- `summary.baseline_violations`: Count of current violations matched to baseline
- `summary.new_violations`: Count of violations not in baseline

These support CI dashboards and governance tracking.
