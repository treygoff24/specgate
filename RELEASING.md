# Releasing Specgate

This document defines how to cut a Specgate release candidate and final release.

## Versioning

Specgate uses [Semantic Versioning](https://semver.org/) (`MAJOR.MINOR.PATCH`).

- **MAJOR**: breaking user-facing changes (CLI contracts, spec compatibility, output contracts).
- **MINOR**: backward-compatible feature additions.
- **PATCH**: backward-compatible fixes and documentation-only corrections.

Current planned release line for Boundary Contracts V2 is `0.2.0`.

## Release checklist

1. Ensure the working tree is clean and pinned to the intended release branch/tag commit.
2. Run merge/release readiness gates:
   - `./scripts/ci/mvp_gate.sh`
   - `specgate policy-diff --base origin/main`
   - `specgate doctor ownership --project-root .`
3. Confirm the changelog is updated for the release:
   - `CHANGELOG.md`
4. Build release binaries with lockfile reproducibility:
   - `cargo build --release --locked`
5. Run smoke checks on the built binary:
   - `./target/release/specgate --version`
6. Generate SHA-256 checksums for distributable artifacts:
   - `shasum -a 256 ./target/release/specgate`
7. Create an annotated tag for the release (for example `v0.2.0-rc1`):
   - `git tag -a v0.2.0-rc1 -m "Specgate v0.2.0-rc1"`
8. Push branch + tag and publish release artifacts/notes in CI.

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
