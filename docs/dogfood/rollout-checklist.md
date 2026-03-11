# Specgate Dogfood Rollout Checklist

## Pre-launch prerequisites

Examples below use `origin/main` as shorthand for the consumer repo's default
branch ref. Substitute your actual default branch ref when it differs.

1. Confirm merge-gate pass (`./scripts/ci/mvp_gate.sh`) on the release commit.
2. Verify baseline generated from clean mainline after gate pass.
3. Confirm consumer workflow exists in target repos:
   - copy `docs/examples/specgate-consumer-github-actions.yml`
   - `fetch-depth: 0` for blast-radius mode
   - include a policy governance step (`specgate policy-diff --base origin/main`)
   - if you choose in-band enforcement, use `specgate check --since origin/main --deny-widenings` instead of a separate policy-diff gate
   - include SARIF upload guidance (`specgate ... --format sarif` + `github/codeql-action/upload-sarif@v3`)
   - include an ownership diagnostics step (`specgate doctor ownership --project-root . --format json`); `strict_ownership: true` controls whether findings block the gate
4. Confirm required docs are discoverable from `README.md`.

## Week 0 (pilot)

- Run on one repo with existing TypeScript modules.
- Enforce deterministic `--output-mode deterministic` in CI.
- Track baseline creation and any drift in `summary.stale_baseline_entries`.
- Verify spec violations flow to GitHub code scanning when SARIF is enabled.

## Week 1–2 (steady-state)

1. Enable blast-radius mode for PR checks (`--since origin/main`).
2. Require `baseline` updates only through PR-approved maintenance windows.
   - keep governance enforcement singular: either `specgate policy-diff --base origin/main` or `specgate check --since origin/main --deny-widenings`.
   - The consumer workflow does not auto-commit baseline files.
   - Run `specgate baseline generate --project-root . --output .specgate-baseline.json` in planned maintenance PRs and commit manually.
3. Validate no new hard failures from `C02/C06/C07` deferred rule classes.
4. File issues for explicit unsupported cases rather than local workarounds.

## Ongoing

- Reconcile stale entries weekly.
- Revisit baseline size and debt trend.
- Review `golden_corpus` outputs as future-proxy evidence, not gate failure.
- Keep this checklist as the default onboarding prerequisite for new consumers.

## Rollback decision criteria

Pause rollout if any condition persists for 2 weeks:

- repeat false positives from stable fixtures;
- unresolved stale baseline growth trend;
- >1 CI incident/week due to baseline regeneration conflict.
