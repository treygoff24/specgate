# Specgate TS/JS v1 Support Matrix

This matrix defines support commitments for the TypeScript and JavaScript v1 surface of Specgate across GitHub binary artifacts and the npm wrapper package.

Here, `v1` refers to the TS/JS support-policy surface, not the `.spec.yml` schema version.

For channel semantics, see [DOGFOOD_RELEASE_CHANNEL](../dogfood/release-channel.md).

> **Note:** This document is the authoritative source for tier definitions. For detailed promotion gates and semver policy, see [DOGFOOD_RELEASE_CHANNEL](../dogfood/release-channel.md).

## Tier Definitions

| Tier | Targets | Channel Availability | Release Gating | Support Expectation |
| --- | --- | --- | --- | --- |
| Tier 1 | `x86_64-unknown-linux-gnu`, `aarch64-apple-darwin`; npm wrapper on Node.js 20/22 LTS | GitHub binaries: Stable + Beta; npm wrapper: Stable only | Must pass release gate, binary asset verification, and npm wrapper publish/verify before promotion | Highest-priority support; Tier-1 regressions are release-blocking |
| Tier 2 | `x86_64-apple-darwin`; npm wrapper on current Node.js (non-LTS) | GitHub binaries: Stable + Beta; npm wrapper: Stable only | Built and smoke-tested in release automation; can proceed only when Tier-1 remains green | Prioritized fixes; known issues must be documented at release time |
| Tier 3 | `aarch64-unknown-linux-gnu`, other OS/arch combinations, source-only installs, and unlisted runtime combinations | Beta-first, stable best-effort | Not part of release-blocking matrix | Best-effort support with documented workarounds |

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

## Tag and Automation Mapping

| Tag Shape | Channel | GitHub Release Type | Binary Workflow | npm Wrapper Workflow |
| --- | --- | --- | --- | --- |
| `vX.Y.Z` | stable | non-prerelease | `.github/workflows/release-binaries.yml` publishes binaries and checksum assets | `.github/workflows/release-npm-wrapper.yml` publishes wrapper to `latest` and verifies dist-tag |
| `vX.Y.Z-beta.N` (or any semver prerelease) | beta | prerelease | `.github/workflows/release-binaries.yml` publishes binaries and marks release as prerelease | `.github/workflows/release-npm-wrapper.yml` skips npm publish for prereleases (binary-only distribution) |
