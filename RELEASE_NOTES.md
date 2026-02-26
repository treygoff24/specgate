# Specgate MVP Release Notes (Dogfood Closeout)

**Release date:** 2026-02-26

## Status

Specgate is in a dogfood-ready MVP state after closeout hardening and operational docs.
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
