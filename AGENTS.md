# Specgate Agent Guide

Specgate is a Rust CLI for enforcing architectural boundaries in TS/JS repos. Treat output shape, fixture behavior, and golden files as user-facing contracts.

## Quick Start

- Use the repo root `README.md` as the operator-facing source of truth.
- Primary verification: `cargo test`
- Contract suites: `cargo test contract_fixtures`, `cargo test tier_a_golden`, `cargo test golden_corpus`

## Working Rules

- Keep CLI output deterministic and CI-safe. Avoid incidental ordering changes, unstable formatting, or environment-dependent verdicts.
- When changing parsing, graph logic, or verdict rendering, update the matching fixture or golden coverage in the same change.
- Treat fixture and golden tests as product contracts, not disposable snapshots.
- If a change affects CLI flags, verdict wording, or install/release behavior, update the operator docs in `README.md` in the same patch.
- Prefer narrow Rust changes over broad policy rewrites unless the spec explicitly requires it.

## Repo Notes

- This repo is Rust-first. Do not introduce Node-specific workflow assumptions into the core development loop.
- If a scope change touches rule semantics, document the intended before/after behavior in the PR or ticket notes before editing fixtures.

## Conventions

### Single-Canonical-Config for Governance Rules

When multiple modules declare the same governance constraint rule (e.g., `enforce-layer`, `enforce-category`) with different parameters, the engine resolves the conflict deterministically:

1. Constraints are sorted by `(module_id, params.to_string())`.
2. Duplicate `(module, params)` pairs are deduplicated.
3. The first valid config (by lexicographic module ID) becomes the canonical config for the entire rule.
4. Remaining configs with different params are reported as config issues (non-fatal warnings in the doctor output).

This means the "winning" config is always the one from the module whose ID sorts first alphabetically. This convention applies to all governance rules that use global config (currently `enforce-layer` and `enforce-category`).
