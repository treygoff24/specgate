# Specgate Release Channels (Dogfood)

## Channels

- **dogfood**
  - Audience: internal pilot repos and early adopters.
  - Scope: mandatory merge gate + explicit baseline policy.
  - Promise: fastest iteration, minimal compatibility guarantees.

- **pre-release**
  - Audience: broader controlled cohort.
  - Scope: merge gate + release-note review + rollout checklist validation.
  - Promise: stabilized docs and predictable baselines across multiple repos.

- **stable**
  - Audience: all supported users.
  - Scope: after two stable dogfood windows meeting success metrics.
  - Promise: explicit support and regular release cadence.

## Upgrade policy

- Patch updates: backward-compatible CLI/config behavior.
- Minor updates: rule expansions and guardrail changes with release-note callouts.
- Breaking updates: version bump aligned with `Cargo.toml` and `WAVE0_CONTRACT.md`.

## Release cadence target

- Tag a release candidate after baseline and gate verification.
- Hold in `dogfood` for one full week.
- Promote to `pre-release` when rollout checklist is complete.
- Promote to `stable` when metrics pass for two consecutive windows.

## Support model

- `dogfood`: best-effort support via implementation notes.
- `pre-release`: prioritized issue triage.
- `stable`: documented support commitments and release notes for every change.
