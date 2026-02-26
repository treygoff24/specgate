# Releasing Specgate

## Scope

This document defines the release mechanics for Specgate MVP dogfood and the
subsequent cadence toward broader adoption.

## Version and artifacts

- Current tracked version in `Cargo.toml`: `0.1.0`.
- Versionable artifact set:
  - `CHANGELOG.md`
  - `RELEASE_NOTES.md`
  - `LICENSE`
  - `RELEASING.md`
  - `docs/BASELINE_POLICY.md`
  - `docs/DOGFOOD_ROLLOUT_CHECKLIST.md`
  - `docs/DOGFOOD_SUCCESS_METRICS.md`
  - `docs/DOGFOOD_RELEASE_CHANNEL.md`
  - `docs/examples/specgate-consumer-github-actions.yml`

## Release prerequisites (dogfood-ready)

Before cutting a release:

1. Clean workspace and run the canonical gate script:
   - `./scripts/ci/mvp_gate.sh`
2. Confirm docs links resolve and the canonical docs references are current.
3. Confirm `LICENSE` matches the SPDX declaration (`MIT` in `Cargo.toml`).
4. Confirm no unresolved placeholders remain in touched docs.

Gate command baselines used by `mvp_gate.sh` (locked + strict):

```bash
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --lib
cargo test --locked --test contract_fixtures
cargo test --locked --test golden_corpus_gate
cargo test --locked --test tier_a_golden
cargo test --locked --test integration
cargo test --locked --test wave2c_cli_integration
cargo test --locked --test mvp_gate_baseline
./scripts/ci/mvp_gate.sh
rg -n "TODO|placeholder|TBD|not wired" README.md docs/*.md
```

## Release note requirements

- Update `CHANGELOG.md` under `[Unreleased]` and add/refresh the next release
  section.
- Update `RELEASE_NOTES.md` with scope, known limitations, and rollout
  expectations.
- Tag a release commit only after merge-gate pass and baseline update.

## Tagging and communication

1. Merge all merge-gate required changes.
2. Create a signed tag (for example `v0.1.0-rc2`).
3. Publish release artifacts in CI/release pipeline.
4. Publish release notes and announce the selected release channel.

## Release-binaries workflow

- File: `.github/workflows/release-binaries.yml`
- Triggers
  - `push` on tags matching `v*`
  - `workflow_dispatch` with required input `tag` (for example `v1.2.3`)
- Behavior
  - Checkout at the tag and run `scripts/ci/mvp_gate.sh`.
  - Validate tag format and that `v$(cat Cargo.toml version)` matches the tag base.
  - Build and package artifacts for:
    - `x86_64-unknown-linux-gnu`
    - `x86_64-apple-darwin`
    - `aarch64-apple-darwin`
  - For each target publish:
    - `specgate-$TAG-<target>.tar.gz`
    - `specgate-$TAG-<target>.tar.gz.sha256`

## Rollout policy (minimum)

- Use `dogfood` channel by default until explicit stability threshold is met.
- Promote to broader consumer use only after the dogfood checklist and
  success metrics are met for two consecutive release windows.

## Post-release verification

1. Confirm all expected assets are present in the release:
   - `specgate-vX.Y.Z-*-unknown-linux-gnu.tar.gz`
   - `specgate-vX.Y.Z-*-apple-darwin.tar.gz`
   - `specgate-vX.Y.Z-*.tar.gz.sha256`
2. Spot-check checksums:

   ```bash
   sha256sum -c specgate-v0.1.0-rc2-aarch64-apple-darwin.tar.gz.sha256
   ```

3. Download the Linux or macOS artifact, unpack, and run a smoke check:

   ```bash
   tar -xzf specgate-v0.1.0-rc2-aarch64-apple-darwin.tar.gz
   ./specgate --version
   ```

## Contacts and ownership

- Docs and process: repository maintainers.
- Gate quality signals: implementation owners.
- Baseline hygiene: designated operator team.
