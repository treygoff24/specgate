# Specgate Release Channels (TS/JS v1)

This document defines release-channel behavior for the TypeScript/JavaScript v1 distribution surface (GitHub binaries + npm wrapper package).

For authoritative channel semantics, tier definitions, and downgrade behavior, see [support-matrix-v1](support-matrix-v1.md). This document focuses on semver policy and promotion gates.

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
- Governance hygiene on `.spec.yml` changes:
  - `specgate policy-diff --base origin/main`
- Ownership diagnostics gate:
  - `specgate doctor ownership --project-root .`
  - enforce the output in CI either by `strict_ownership: true` or explicit release approval
- Binary artifact build and verification
- npm wrapper publish and dist-tag verification

Promote beta to stable only after all gate checks are green, `policy-diff`/ownership diagnostics are clean, and artifact and wrapper publish/verify checks pass.

## Tag and Automation Mapping

For tag/automation mapping details, see [support-matrix-v1.md](../reference/support-matrix-v1.md#tag-and-automation-mapping).

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
- Full downgrade policy and tier commitments live in [support-matrix-v1](../reference/support-matrix-v1.md).
