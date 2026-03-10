# Specgate for Agents

If you are an agent helping a user install, evaluate, or integrate Specgate, start here.

## What Specgate Is

Specgate is a Rust CLI that enforces architectural boundaries in TypeScript and JavaScript repositories.

It checks things like:
- which modules may import which other modules,
- which files are public API,
- whether policy changes widen or narrow architecture rules,
- whether ownership coverage is missing or overlapping.

The CLI is designed for deterministic CI use. Output shape, fixture behavior, and golden files are treated as product contracts.

## What It Is For

Use Specgate when a repo needs:
- architectural boundary enforcement,
- deterministic CI gating for import-policy violations,
- policy-governance checks across `.spec.yml` changes,
- ownership diagnostics for module/spec coverage.

Do not position it as a generic linter, formatter, or type checker. It complements those tools.

## Current Release

- Current published release tag in this repo: `v0.3.0`
- Source of truth for the release version: `Cargo.toml`
- Changelog: [CHANGELOG.md](CHANGELOG.md)

## Install

Preferred path:
- install the published release artifact for an explicit tag and verify its `.sha256` checksum before use

Fallback path:

```bash
cargo install --locked --git https://github.com/treygoff24/specgate --tag v0.3.0
```

For the full release-asset flow, use:
- [README.md](README.md)
- [docs/reference/getting-started.md](docs/reference/getting-started.md)

## First Commands to Reach For

Initialize a consumer repo:

```bash
specgate init
```

Validate spec syntax and config:

```bash
specgate validate
```

Run the main policy check:

```bash
specgate check --output-mode deterministic
```

Detect policy widenings between refs:

```bash
specgate policy-diff --base origin/main
```

Use in-band widening enforcement instead of a separate policy-diff step:

```bash
specgate check --since origin/main --deny-widenings
```

In consumer-repo examples, `origin/main` is shorthand for the target repo's
default branch ref. Replace it with the real base ref when needed (for example
`origin/master`).

Inspect ownership coverage:

```bash
specgate doctor ownership --project-root . --format json
```

## Core Concepts Agents Should Know

- Specs live in `.spec.yml` files, usually under `modules/`.
- Supported spec versions are `2.2` and `2.3`.
- `2.3` is required when using boundary contracts.
- Governance CI should pick exactly one widening gate:
  - `specgate policy-diff --base <ref>`, or
  - `specgate check --since <ref> --deny-widenings`
- SARIF output is available with `specgate check --format sarif`.
- Ownership findings only become a blocking gate when `strict_ownership: true` is enabled in `specgate.config.yml`.

## Canonical Docs

Send agents here depending on the task:

- [README.md](README.md): top-level overview, install options, CI guidance
- [docs/reference/operator-guide.md](docs/reference/operator-guide.md): best single onboarding document
- [docs/reference/getting-started.md](docs/reference/getting-started.md): copy-paste setup path
- [docs/reference/spec-language.md](docs/reference/spec-language.md): spec file format and version rules
- [docs/reference/policy-diff.md](docs/reference/policy-diff.md): governance diff semantics and exit codes
- [docs/reference/sarif-github-actions.md](docs/reference/sarif-github-actions.md): SARIF integration
- [docs/examples/specgate-consumer-github-actions.yml](docs/examples/specgate-consumer-github-actions.yml): consumer CI template

## Important Repo-Specific Note

If you are working inside the Specgate repo itself, do not treat `specgate check` at repo root as a normal consumer-repo signal.

This repository contains intentionally invalid and duplicate fixture specs under `tests/fixtures/` for contract coverage. Those fixtures can make self-directed `specgate check` or `specgate doctor ownership` runs fail validation even when the product is healthy.

For validating the Specgate product repo itself, prefer:

```bash
./scripts/ci/mvp_gate.sh
cargo test
cargo build --release --locked
```

## Short Agent Prompt

If a user asks you to install or integrate Specgate, start with this file, then move to the operator guide and the getting-started guide before making CI or policy recommendations.
