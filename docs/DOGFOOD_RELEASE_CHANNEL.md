# Specgate Release Channels (Dogfood)

## Channels

- **dogfood**
  - Audience: internal pilot repos and early adopters.
  - Scope: mandatory merge gate + explicit baseline policy.
  - Promise: fastest iteration, minimal compatibility guarantees.

- **pre-release**
  - Audience: broader controlled cohort.
  - Scope: merge gate + release-note review + rollout checklist validation.
  - Promise: stabilized docs and predictable baselines across multiple repos.

- **stable**
  - Audience: all supported users.
  - Scope: after two stable dogfood windows meeting success metrics.
  - Promise: explicit support and regular release cadence.

## Consumer install preference

- Default install path for released versions is the prebuilt release artifact (fast path):
  - `specgate-<tag>-x86_64-unknown-linux-gnu.tar.gz`
  - `specgate-<tag>-x86_64-unknown-linux-gnu.tar.gz.sha256`
- Verify checksum before unpacking, then assert the extracted `specgate` binary exists and is executable before adding it to `PATH`.
- Use resilient release asset fetches with retries and bounded connect/overall timeouts.
- In fallback mode, install from source with an isolated root (`--root "$RUNNER_TEMP/specgate-install/cargo-root" --force`) and add `cargo-root/bin` to `PATH` to avoid runner-path ambiguity.
- Keep `cargo install --locked --git https://github.com/treygoff24/specgate --tag <tag>` as fallback when release assets are not available, with the hardened root/path behavior above.
- Example release tag for initial dogfood: `v0.1.0-rc2`.
- Release verification contract for that tag is `.github/workflows/release-asset-verify.yml`, run after publish:
  - pass requires checksum verification for all published artifacts
  - pass requires `specgate --version` smoke checks for each target artifact
- For `v0.1.0-rc2` and subsequent dogfood releases, only promote after that workflow is green.

## Upgrade policy

- Patch updates: backward-compatible CLI/config behavior.
- Minor updates: rule expansions and guardrail changes with release-note callouts.
- Breaking updates: version bump aligned with `Cargo.toml` and `WAVE0_CONTRACT.md`.

## Release cadence target

- Tag a release candidate after baseline and gate verification.
- Hold in `dogfood` for one full week.
- Promote to `pre-release` when rollout checklist is complete.
- Promote to `stable` when metrics pass for two consecutive windows.

## Support model

- `dogfood`: best-effort support via implementation notes.
- `pre-release`: prioritized issue triage.
- `stable`: documented support commitments and release notes for every change.
