# Specgate MVP Release Notes (Dogfood Closeout)

**Release date:** 2026-02-26

## Status

Specgate is in a dogfood-ready MVP state, operating on the `dogfood` channel at `v0.1.0-rc3`.
Core validation is stable and deterministic for the current enforced scope.

## What is included

- Deterministic architecture checks in CI with stable fixture contracts.
- Baseline fingerprinting with explicit new vs baseline violation handling.
- Canonical import enforcement, dependency boundaries, provider/importer precedence
  and graph-level cycle controls.
- Resolver parity command (`specgate doctor compare`) for targeted diagnostics.
- Full closeout documentation set:
  - `LICENSE` (MIT)
  - `CHANGELOG.md`
  - `RELEASING.md`
  - `docs/BASELINE_POLICY.md`
  - `docs/DOGFOOD_ROLLOUT_CHECKLIST.md`
  - `docs/DOGFOOD_SUCCESS_METRICS.md`
  - `docs/DOGFOOD_RELEASE_CHANNEL.md`
  - `docs/examples/specgate-consumer-github-actions.yml`

## Explicit MVP limitations (deferred work)

- `C02` pattern-aware checks and `C07` unique-export/visibility variants are
  deferred to post-MVP hardening until deterministic fixture coverage is added.
- `C06` category-level governance checks are outside the Tier A dogfood gate.
  They remain validated as future-proxy/informational coverage.
- `golden_corpus` remains informational and is not part of the enforced merge gate.

## Operator impact

- Follow [BASELINE_POLICY](docs/BASELINE_POLICY.md) for stale-baseline
  lifecycle and triage.
- Use the rollout docs for adoption cadence and release communications:
  - [DOGFOOD_ROLLOUT_CHECKLIST](docs/DOGFOOD_ROLLOUT_CHECKLIST.md)
  - [DOGFOOD_SUCCESS_METRICS](docs/DOGFOOD_SUCCESS_METRICS.md)
  - [DOGFOOD_RELEASE_CHANNEL](docs/DOGFOOD_RELEASE_CHANNEL.md)

## RC3 hardening since initial closeout

- Merge gate hardening in `scripts/ci/mvp_gate.sh`:
  - Added/kept lockfile gating (`--locked`) on gate commands.
  - Explicitly enforces `cargo test --lib`, `cargo test --test golden_corpus_gate`, and `cargo test --test integration` as part of the enforced merge scope.
  - Continues policy enforcement of new violations via `mvp_gate_baseline`.
- Release publication hardening:
  - `.github/workflows/release-binaries.yml` now validates tag format/version alignment, runs merge gate before publish, checks artifact completeness, and performs per-target smoke checks.
  - `.github/workflows/release-asset-verify.yml` was added to validate checksums and execute `specgate --version` for each released target after publish.
- Consumer installation/operations hardening:
  - `docs/examples/specgate-consumer-github-actions.yml` verifies release checksum and executable presence before using prebuilt binaries.
  - Includes `.specgate-verdict.json` telemetry summarization.
  - Uses `cargo install --locked --git ... --tag ... --root ...` fallback with explicit executable checks.
- Evidence continuity:
  - Added RC gate-evidence snapshots for `v0.1.0-rc1`, `v0.1.0-rc2`, and `v0.1.0-rc3` in `docs/release-artifacts/`.
