# Changelog

All notable project changes are documented here. Dates are UTC.

## [Unreleased]

- No unreleased code changes are currently queued in this repository snapshot.
- Dogfood-release documentation set added in parallel with closeout of MVP hardening.

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
