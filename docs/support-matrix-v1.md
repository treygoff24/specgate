# Specgate TS/JS v1 Support Matrix

This matrix defines support commitments for the TypeScript/JavaScript v1 surface of Specgate across GitHub binary artifacts and the npm wrapper package.

## Channel Semantics

- **Stable**
  - Tag format: `vMAJOR.MINOR.PATCH` (for example `v1.2.3`)
  - GitHub release type: non-prerelease
  - npm wrapper dist-tag: `latest`
  - Intended use: default production channel.

- **Beta**
  - Tag format: `vMAJOR.MINOR.PATCH-<prerelease>` (for example `v1.2.3-beta.1`)
  - GitHub release type: prerelease
  - npm wrapper dist-tag: `beta`
  - Intended use: dogfood and pre-production validation.

## Tier Definitions

| Tier | Targets | Channel Availability | Release Gating | Support Expectation |
| --- | --- | --- | --- | --- |
| Tier 1 | `x86_64-unknown-linux-gnu`, `aarch64-apple-darwin`; npm wrapper on Node.js 20/22 LTS | Stable + Beta | Must pass release gate, binary asset verification, and npm wrapper publish/verify before promotion | Highest-priority support; Tier-1 regressions are release-blocking |
| Tier 2 | `x86_64-apple-darwin`; npm wrapper on current Node.js (non-LTS) | Stable + Beta | Built and smoke-tested in release automation; can proceed only when Tier-1 remains green | Prioritized fixes; known issues must be documented at release time |
| Tier 3 | Other OS/arch combinations, source-only installs, and unlisted runtime combinations | Beta-first, stable best-effort | Not part of release-blocking matrix | Best-effort support with documented workarounds |

## Downgrade Behavior

1. Tier-1 regression before release publish:
   - Block stable promotion.
   - Keep the candidate in beta and cut a new beta tag after fix.
2. Tier-1 regression after stable publish:
   - Treat as a release incident.
   - Ship an emergency patch and pause further stable promotion until verification is green.
3. Tier-2 regression:
   - Stable may proceed only if Tier-1 is green and release notes include the regression and mitigation.
   - If unresolved for two consecutive releases, temporarily downgrade the affected target to Tier 3.
4. Tier-3 regression:
   - Track as best-effort and publish workaround guidance; does not block stable releases.

## Promotion Rule

Promote a beta line to stable only after all of the following are green for the candidate version:

- merge/release gate checks,
- binary artifact verification, and
- npm wrapper publish + dist-tag verification.

## Automation Alignment

- `.github/workflows/release-binaries.yml` publishes multi-platform binaries and marks prerelease tags as beta releases.
- `.github/workflows/release-npm-wrapper.yml` publishes the npm wrapper and verifies `latest` (stable) or `beta` dist-tag alignment.
