# Releasing Specgate

This document defines how to cut a Specgate release candidate and final release.

## Versioning

Specgate uses [Semantic Versioning](https://semver.org/) (`MAJOR.MINOR.PATCH`).

- **MAJOR**: breaking user-facing changes (CLI contracts, spec compatibility, output contracts).
- **MINOR**: backward-compatible feature additions.
- **PATCH**: backward-compatible fixes and documentation-only corrections.

Use `Cargo.toml` as the single source of truth for the release version and tag.

## Release checklist

1. Ensure the working tree is clean and pinned to the intended release branch/tag commit.
2. Run merge/release readiness gates:
   - `./scripts/ci/mvp_gate.sh`
   - Governance gate for the Specgate repo release commit:
     - `cargo run --quiet -- policy-diff --base origin/master --format json`
   - Confirm the consumer-facing workflow/docs remain aligned:
     - `README.md`
     - `docs/examples/specgate-consumer-github-actions.yml`
     - `docs/reference/sarif-github-actions.md`
     - `docs/reference/operator-guide.md`
3. Confirm release notes and upgrade guidance are aligned for the release:
   - `CHANGELOG.md`
   - `README.md`
   - `docs/roadmap.md`
   - `RELEASING.md`
4. Build release binaries with lockfile reproducibility:
   - `cargo build --release --locked`
5. Run smoke checks on the built binary:
   - `./target/release/specgate --version`
6. Generate SHA-256 checksums for distributable artifacts:
   - `shasum -a 256 ./target/release/specgate`
7. Create an annotated tag for the release (for example `vX.Y.Z-rc1`):
   - `git tag -a vX.Y.Z-rc1 -m "Specgate vX.Y.Z-rc1"`
8. Push branch + tag and publish release artifacts/notes in CI.

## Repo-specific note

When releasing Specgate itself, do not treat repo-root `specgate check` or
`specgate doctor ownership` runs as release blockers. This repository contains
intentionally invalid and duplicate fixture specs under `tests/fixtures/` for
contract coverage, so those commands can fail validation even when the product
and release commit are healthy.

Use `./scripts/ci/mvp_gate.sh`, the governance diff against `origin/master`,
the locked release build, and the binary smoke check as the authoritative
release gates for this repo.

## Reproducibility

Always use a locked dependency graph when producing release artifacts:

```bash
cargo build --release --locked
```

This ensures Cargo.lock-resolved dependencies are used exactly as tested in CI.

## Checksums

For each produced release artifact, publish a SHA-256 checksum file or value.

Example for local binary verification:

```bash
shasum -a 256 ./target/release/specgate
```

Store checksum outputs alongside release artifacts so consumers can verify integrity before execution.
