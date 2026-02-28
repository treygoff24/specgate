# Specgate Release Channels (TS/JS v1)

This document defines release-channel behavior for the TypeScript/JavaScript v1 distribution surface (GitHub binaries + npm wrapper package).

## Channel Contract

- **stable**
  - Tag format: `vMAJOR.MINOR.PATCH`
  - Audience: production users
  - Promise: full support commitment for Tier-1/Tier-2 targets
  - npm wrapper dist-tag: `latest`

- **beta**
  - Tag format: `vMAJOR.MINOR.PATCH-<prerelease>`
  - Audience: dogfood users and early adopters
  - Promise: faster iteration with stricter rollback expectations
  - npm wrapper dist-tag: `beta`

## Tag and Automation Mapping

| Tag Shape | Channel | GitHub Release Type | Binary Workflow | npm Wrapper Workflow |
| --- | --- | --- | --- | --- |
| `vX.Y.Z` | stable | non-prerelease | `.github/workflows/release-binaries.yml` publishes binaries and checksum assets | `.github/workflows/release-npm-wrapper.yml` publishes wrapper to `latest` and verifies dist-tag |
| `vX.Y.Z-beta.N` (or any semver prerelease) | beta | prerelease | `.github/workflows/release-binaries.yml` publishes binaries and marks release as prerelease | `.github/workflows/release-npm-wrapper.yml` publishes wrapper to `beta` and verifies dist-tag |

## Install Preference

- Preferred install path is the matching release artifact + checksum:
  - `specgate-<tag>-x86_64-unknown-linux-gnu.tar.gz`
  - `specgate-<tag>-x86_64-unknown-linux-gnu.tar.gz.sha256`
- Verify checksum before unpacking and ensure `specgate --version` succeeds.
- Keep `cargo install --locked --git https://github.com/treygoff24/specgate --tag <tag>` as fallback when release assets are unavailable.

## Promotion and Downgrade Rules

- Beta is the default dogfood channel for TS/JS v1 changes.
- Promote beta to stable only after gate, artifact verification, and wrapper publish/verify checks are green.
- If a Tier-1 regression appears, pause stable promotion and continue on beta until fixed.
- Full downgrade policy and tier commitments live in [support-matrix-v1](support-matrix-v1.md).
