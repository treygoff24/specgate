# Changelog

All notable project changes are documented here. Dates are UTC.

## [Unreleased]

- Pinned release and gate tooling on Rust `1.88.0` and enforced lockfile usage where it matters (`--locked`) in merge-gate and release commands.
- Hardened merge-gate scope in `scripts/ci/mvp_gate.sh` to include `cargo test --locked --lib`, `cargo test --locked --test golden_corpus_gate`, and integration suites alongside existing contract/baseline checks.
- Strengthened release-binaries workflow (`.github/workflows/release-binaries.yml`) with tag/version validation, pre-publish `mvp_gate.sh` gating, expected artifact completeness checks, and binary smoke checks (`specgate --version`).
- Added `.github/workflows/release-asset-verify.yml` to validate published checksums and run per-target binary smoke checks after release publication.
- Consumer workflow hardened for release adoption:
  - prebuilt install verifies checksums and executable integrity before use,
  - emits Specgate telemetry summary from `specgate-verdict.json`,
  - keeps baseline updates manual and uses fallback `cargo install --locked --git --tag <tag> --root ...` when assets are unavailable.
- Added explicit release hardening evidence path (`docs/release-artifacts/`) for `v0.1.0-rc1`, `v0.1.0-rc2`, and `v0.1.0-rc3` with RC3 as current dogfood target.

## [0.1.0] - 2026-02-26

### Added
- Deterministic CI-grade verdicts with byte-identical output (`deterministic` mode).
- Baseline lifecycle artifacts with fingerprinted violations and new-vs-baseline
  classification in merge-gating summaries.
- Resolver parity diagnostics (`specgate doctor compare`).
- Rule stack for precedence + provider/importer constraints.
- Tier A deterministic fixture set and merge-gate harness.
- Release documentation bundle: [CHANGELOG.md], [RELEASE_NOTES.md], [RELEASING.md],
  and dogfood operation docs in `docs/`.

### Changed
- Consolidated operator guidance to align rule precedence and CI behavior.
- Updated merge-gate narrative to distinguish enforced vs informational checks.

### Fixed
- Clarified stale-baseline behavior and explicit failure modes for dogfood rollout.
