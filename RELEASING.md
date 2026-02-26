# Releasing Specgate

## Scope

This document defines the release mechanics for Specgate MVP dogfood and the
subsequent cadence toward broader adoption.

## Version and artifacts

- Current tracked version in `Cargo.toml`: `0.1.0`.
- Versionable artifact set:
  - `CHANGELOG.md`
  - `RELEASE_NOTES.md`
  - `LICENSE`
  - `RELEASING.md`
  - `docs/BASELINE_POLICY.md`
  - `docs/DOGFOOD_ROLLOUT_CHECKLIST.md`
  - `docs/DOGFOOD_SUCCESS_METRICS.md`
  - `docs/DOGFOOD_RELEASE_CHANNEL.md`
  - `docs/examples/specgate-consumer-github-actions.yml`

## Release prerequisites (dogfood-ready)

Before cutting a release:

1. Clean workspace and run the merge gate sequence.
2. Confirm docs links resolve and the canonical docs references are current.
3. Confirm `LICENSE` matches the SPDX declaration (`MIT` in `Cargo.toml`).
4. Confirm no unresolved placeholders remain in touched docs.

Recommended command baseline:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
./scripts/ci/mvp_gate.sh
rg -n "TODO|placeholder|TBD|not wired" README.md docs/*.md
```

## Release note requirements

- Update `CHANGELOG.md` under `[Unreleased]` and add/refresh the next release
  section.
- Update `RELEASE_NOTES.md` with scope, known limitations, and rollout
  expectations.
- Tag a release commit only after merge-gate pass and baseline update.

## Tagging and communication

1. Merge all merge-gate required changes.
2. Create a signed tag (e.g. `v0.1.0-rc.1` for dogfood candidates).
3. Publish release artifacts in CI/release pipeline.
4. Publish release notes and announce the selected release channel.

## Rollout policy (minimum)

- Use `dogfood` channel by default until explicit stability threshold is met.
- Promote to broader consumer use only after the dogfood checklist and
  success metrics are met for two consecutive release windows.

## Contacts and ownership

- Docs and process: repository maintainers.
- Gate quality signals: implementation owners.
- Baseline hygiene: designated operator team.
