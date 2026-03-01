# Specgate Release Channels (TS/JS v1)

This document defines release-channel behavior for the TypeScript/JavaScript v1 distribution surface (GitHub binaries + npm wrapper package).

For authoritative channel semantics and tier definitions, see [support-matrix-v1](support-matrix-v1.md).

## Semver Upgrade Policy

| Change Type | Increment | When to Apply |
| --- | --- | --- |
| **Patch** | PATCH | Bug fixes, perf improvements, docs updates, internal refactors with no behavioral change |
| **Minor** | MINOR | New features, deprecated functionality (with warning), internal improvements |
| **Major** | MAJOR | Breaking changes to public APIs, CLI flags, output formats, or file locations |

CI-gating tool consumers should expect this contract: any version bump follows the above rules, and breaking changes are never introduced in patch or minor releases.

## Promotion Gate

The "gate" refers to the merge gate and CI pipeline that must pass before beta can promote to stable:

- All merge-required status checks (CI tests, linting, type checking)
- Binary artifact build and verification
- npm wrapper publish and dist-tag verification

Promote beta to stable only after all gate checks are green, artifact verification succeeds, and wrapper publish/verify checks pass.

## Tag and Automation Mapping

| Tag Shape | Channel | GitHub Release Type | Binary Workflow | npm Wrapper Workflow |
| --- | --- | --- | --- | --- |
| `vX.Y.Z` | stable | non-prerelease | `.github/workflows/release-binaries.yml` publishes binaries and checksum assets | `.github/workflows/release-npm-wrapper.yml` publishes wrapper to `latest` and verifies dist-tag |
| `vX.Y.Z-beta.N` (or any semver prerelease) | beta | prerelease | `.github/workflows/release-binaries.yml` publishes binaries and marks release as prerelease | `.github/workflows/release-npm-wrapper.yml` skips npm publish for prereleases (binary-only distribution) |

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
