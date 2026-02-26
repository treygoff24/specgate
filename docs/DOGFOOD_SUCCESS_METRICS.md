# Specgate Dogfood Success Metrics

## Primary gate health

- Merge gate pass rate on required commands: **>= 98%** target.
- `specgate check` deterministic success on PR paths: **>= 99%** target.
- `specgate check --output-mode deterministic` mean runtime in CI:
  - target: **<= 10% increase** over baseline repo test time.

## Baseline health

- `baseline_violations` should monotonically decrease or stay flat after rollout.
- `stale_baseline_entries` should be reviewed and reduced at least weekly.
- `new_error_violations` should drop to zero within the first two dogfood windows.

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
2. Deferred rule families (`C02`, `C06`, `C07`) remain explicitly documented
   and do not regress into accidental production behavior.
3. No open unresolved blocker in [BASELINE_POLICY](BASELINE_POLICY.md).
