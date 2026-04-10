# Specgate Baseline Policy

## Purpose

Baselines let operators track known violations without turning every CI run into
triage churn. This policy defines how `specgate` baselines are created, reviewed,
and retired in the current product.

## In-scope baseline behavior

- File: `.specgate-baseline.json`
- Storage: checked in at repository root.
- Classification in verdicts:
  - `baseline_violations` (known)
  - `new_violations` (new)
  - `stale_baseline_entries` (expired)
- Gate effect:
  - Baseline matches are report-only.
  - New violations follow normal severity policy.

## Policy decisions

1. **Baseline mismatch does not block merges by default**
   - A stale or missing match still reports in summary fields.
   - It does not convert the run to failure status.

2. **Strict stale policy is opt-in**
   - Set `stale_baseline: fail` in `specgate.config.yml` to make stale baseline
     entries policy-failing.
   - Default is `stale_baseline: warn` so teams can triage before enforcing.

3. **Runbook for stale entries**
   - Re-run `specgate baseline --output .specgate-baseline.json` intentionally
     when baseline debt is reviewed and accepted.
   - Stale entries must be reviewed during each dogfood window.

4. **Owner + cadence**
   - Owner: project operator on-call.
   - Review cadence: weekly.
   - No auto-prune in MVP to keep history traceable.

## Explicit limitations and future semantics

The following capability classes are still deferred and should be treated as explicit
non-guarantees for now:

- `C02` pattern-aware no-pattern style checks are not part of the enforced
  dogfood gate yet.
- `C06` category-level governance variants are roadmap-only.
- `C07` unique-export/visibility edge cases are not yet fully enforced in the
  Tier A gate.

## When a baseline is invalid

Treat the baseline as invalid and regenerate if:

- file format cannot be parsed;
- config/spec hashes are missing;
- environment mismatch makes remediation paths ambiguous.

In these cases, run checks with `--no-baseline` and regenerate with:

```bash
specgate baseline --output .specgate-baseline.json
```
