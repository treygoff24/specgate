# Specgate Dogfood Success Metrics

## Primary gate health

Examples below use `origin/main` as shorthand for the consumer repo's default
branch ref. Substitute your actual default branch ref when it differs.

- Merge gate pass rate on required commands: **>= 98%** target.
- `specgate check` deterministic success on PR paths: **>= 99%** target.
- `specgate check --output-mode deterministic` mean runtime in CI:
  - target: **<= 10% increase** over baseline repo test time.
- `specgate policy-diff --base origin/main` should report clean governance behavior (no unexpected widening).
- `specgate doctor ownership --project-root . --format json` should complete with no unclaimed, overlapping, or orphaned ownership findings before promotion.

## Baseline health

- `baseline_violations` should monotonically decrease or stay flat after rollout.
- `stale_baseline_entries` should be reviewed and reduced at least weekly.
- `new_error_violations` should drop to zero within the first two dogfood windows.
- Consumer workflows now export the raw verdict as `.specgate-verdict.json`.
- The artifact is uploaded as `specgate-verdict` and summarized in `GITHUB_STEP_SUMMARY`.
- Track these telemetry fields each window:
  - `summary.new_error_violations`
  - `summary.baseline_violations`
  - `summary.stale_baseline_entries`
  - `metrics.total_ms`

## Adoption and reliability signals

- Consumer workflow adoption: >`90%` of active repos using provided workflow
- False positive rate: fewer than `1` confirmed false-positive incident per 2
  release windows.
- Operator response SLO:
  - classify and triage baseline-only failures within **1 business day**;
  - resolve policy regressions within **2 business days**.

## Success criteria for promotion

Specgate moves from dogfood to broader release channel only when:

1. All primary gate targets are met for two consecutive windows.
2. Governance remains clean: `policy-diff` output has no unexpected policy widening and deferred classes are handled by process.
3. Ownership diagnostics are clean under the release policy: no open `unclaimed_files`, `overlapping_files`, or `orphaned_specs` findings.
4. No open unresolved blocker in [baseline policy](../design/baseline-policy.md).
