# Specgate

**File-edge structural policy engine with deterministic output contract.**

Specgate enforces architecture boundaries, layer constraints, and dependency rules for TypeScript/JavaScript projects with byte-identical output for CI reliability.

Agent entrypoint: [SPECGATE_FOR_AGENTS.md](SPECGATE_FOR_AGENTS.md)

## Quick Start (First 15 Minutes)

```bash
# 1. Initialize a new project
specgate init

# 2. Create your first spec file
cat > modules/my-module.spec.yml << 'EOF'
version: "2.3"
module: my-module
description: "My first guarded module"
boundaries:
  public_api:
    - src/index.ts
  allow_imports_from:
    - shared/utils
EOF

# 3. Run your first check
specgate check

# 4. See violations (if any)
specgate check --baseline-diff
```

See [First 15 Minutes Guide](docs/reference/getting-started.md#first-15-minutes) for the full walkthrough.

## Baseline Commands

```bash
# Generate or refresh a baseline
specgate baseline generate --project-root .
specgate baseline --project-root . --refresh    # backwards-compatible alias

# Add matching live violations with metadata
specgate baseline add --project-root . --rule boundary.never_imports --from-module app --owner team-app --reason "legacy migration"

# Inspect baseline health
specgate baseline list --project-root . --owner team-app --format json
specgate baseline audit --project-root . --format json
```

## Documentation

### Reference (start here)

| Document | Purpose |
|----------|---------|
| [**Specgate for Agents**](SPECGATE_FOR_AGENTS.md) | **Point agents here** — what Specgate is, how to install it, and which docs to read next |
| [**Operator Guide**](docs/reference/operator-guide.md) | **Start here** — Complete onboarding path |
| [First 15 Minutes](docs/reference/getting-started.md) | Quick hands-on tutorial |
| [Spec Language Reference](docs/reference/spec-language.md) | YAML spec file format |
| [Policy diff reference](docs/reference/policy-diff.md) | Compare policy/config snapshots across git refs and detect widenings |
| [MVP Merge Gate](docs/reference/mvp-merge-gate.md) | Single merge-ready gate definition |
| [TS/JS v1 Support Matrix](docs/reference/support-matrix-v1.md) | Tier 1/2/3 commitments, downgrade rules, stable/beta semantics |

### Design

| Document | Purpose |
|----------|---------|
| [Boundary Contracts V2](docs/design/boundary-contracts-v2.md) | Contract model, envelope protocol, implementation phases |
| [CI Gate Understanding](docs/design/ci-gate-understanding.md) | How Specgate works in CI |
| [Baseline Policy](docs/design/baseline-policy.md) | Baseline lifecycle and stale-entry policy |
| [Tier A Fixture Design](docs/design/tier-a-fixture-design.md) | Deterministic CI gate contract |

### Dogfood

| Document | Purpose |
|----------|---------|
| [Rollout Checklist](docs/dogfood/rollout-checklist.md) | Pre-launch onboarding checklist |
| [Success Metrics](docs/dogfood/success-metrics.md) | Success criteria for dogfood adoption |
| [Release Channel](docs/dogfood/release-channel.md) | Stable/beta channel strategy |

### Project

| Document | Purpose |
|----------|---------|
| [Releasing Guide](RELEASING.md) | Release process and ownership |
| [Changelog](CHANGELOG.md) | Versioned change log |
| [Roadmap](docs/roadmap.md) | Single-source release closeout status |
| [Archive Index](docs/archive/ARCHIVE_INDEX.md) | Superseded plans, reviews, and release artifacts |

## Install options

- Preferred path: download the release tarball + `.sha256` for your tag (for example `vX.Y.Z`) and run the checksum check before using `specgate`.
- Fallback path: `cargo install --locked --git https://github.com/treygoff24/specgate --tag vX.Y.Z`.
- See the full copy-paste command flow in [Getting Started](docs/reference/getting-started.md#minute-0-2-build-and-install).

## Key Concepts

### Modules
Units of architecture (e.g., `core/api`, `features/auth`). Each has a `.spec.yml` defining its boundaries.

### Boundaries
- **`public_api`**: Which files external modules can import from
- **`allow_imports_from`**: Which modules this module can import from
- **`never_imports`**: Modules this module must never import
- **`enforce_canonical_imports`**: Require canonical import IDs, not relative paths

### Verdicts
Deterministic JSON output with pass/fail status, violations, and policy metadata. Byte-identical across runs for same inputs.

## CI Integration

```yaml
# Example GitHub Actions
- name: Specgate Check
  run: |
    specgate check --output-mode deterministic
    # Exit 0 = pass, 1 = policy violation, 2 = runtime error
```

See [CI Gate Understanding](docs/design/ci-gate-understanding.md) for complete CI patterns.

### Policy diff

Examples below use `origin/main` as shorthand for the consumer repo's default
branch ref. Substitute your actual default branch ref when it differs (for
example `origin/master`).

Use `specgate policy-diff --base origin/main` to compare module specs plus `specgate.config.yml` between refs and classify the result as widening, narrowing, or structural. Add `--head <ref>` to compare explicit refs, `--format json` or `--format ndjson` for machine-readable output, and `--cross-file-compensation` when you want opt-in offset analysis for directly connected modules. Exit `0` means no widenings were detected, exit `1` means one or more widenings were detected, and exit `2` means the command could not complete because of a git or parse error.

You can enforce the same governance directly in `check` with `--deny-widenings` when `--since` is provided:

```bash
specgate check --since origin/main --deny-widenings
```

With this flag, widening changes force exit `1`, governance/runtime failures force exit `2`, and non-widening diffs keep normal `check` behavior.

Use exactly one governance gate in CI (`policy-diff` or `check --deny-widenings`) so failures are not duplicated.

For exit code `2`, `policy-diff` keeps structured entries in `errors` but clears authoritative classification payload fields (`diffs`, non-zero summary counters, and top-level `net_classification`) so consumers do not treat partial output as a gate signal.

In the MVP, deleting a `.spec.yml` file is always a widening. Renames/copies use semantic pairing: equivalent snapshots are `structural`, while inconclusive pairings stay fail-closed as widening risk. In CI, fetch full history before diffing against remote refs.

```yaml
- uses: actions/checkout@v4
  with:
    fetch-depth: 0

- name: Detect policy widenings
  run: specgate policy-diff --base origin/main
```

See [Policy diff reference](docs/reference/policy-diff.md) for format details, examples, shallow clone guidance, config-governance behavior, cross-file compensation semantics, and ownership-governance field coverage such as `strict_ownership` and `strict_ownership_level`.

### Upgrade guidance

- Governance: pick one gate path for PRs - `specgate policy-diff --base <ref>` or `specgate check --since <ref> --deny-widenings`.
- SARIF: add `specgate check --format sarif > specgate.sarif` when uploading results to code scanning platforms.
- Ownership diagnostics: add `specgate doctor ownership --project-root . --format json`; the canonical workflow now uploads ownership output. With `strict_ownership: true`, `strict_ownership_level: errors` gates duplicate module ids, invalid ownership globs, and contradictory ownership globs, while `strict_ownership_level: warnings` gates all ownership findings. Both fields are also part of shipped config-governance diffing in `policy-diff`.
- Fetch depth: when diffing against remote refs, use full history (`fetch-depth: 0`).

## Project Status

**Status (as of 2026-03-11): `v0.3.1` is the current release from `master`, with Phase 5 envelope checks, `policy-diff` config governance, opt-in cross-file compensation, `check --deny-widenings`, SARIF output, doctor ownership, monorepo support, adversarial fixtures, and CLI refactor updates all shipped. Current roadmap items are now limited to genuinely unimplemented Tier 2/Tier 3 backlog work beyond the `v0.3.1` release.**

### Completed
- ✅ Envelope validation in Phase 5: contract `envelope` rules, scoped function matching, and static boundary checks.
- ✅ `specgate policy-diff` for policy evolution checks, including deterministic spec/config diffing, semantic rename/copy pairing, and opt-in cross-file compensation.
- ✅ `specgate check --deny-widenings` for single-command governance enforcement when using `--since`.
- ✅ SARIF reporting via `--format sarif` for CI security scanning workflows.
- ✅ `specgate doctor ownership` for ownership diagnostics and strict CI-friendly enforcement, including `strict_ownership_level` runtime thresholds, config-governance coverage, and detection of contradictory ownership globs.
- ✅ `specgate doctor governance-consistency` to detect contradictory namespace-intent across policies.
- ✅ Rule Expansion: Implemented C02 pattern-aware variants, C06 category-level governance checks, and C07 unique-export/visibility boundaries.
- ✅ Import Hygiene: Deep package-internal import hygiene scenario coverage to prevent public-API circumvention.
- ✅ Full monorepo support including workspace discovery, nearest-tsconfig resolution, and `workspace_packages` reporting.
- ✅ Expanded adversarial and parity fixtures plus coverage in contract/golden/CI test sets.
- ✅ CLI refactor work for modular command structure and stable command-level diagnostics.

### Next Steps
- 📌 Keep the remaining Tier 2/Tier 3 backlog explicit in the roadmap and operator references without relabeling shipped governance features as deferred.
- 📌 Use [SPECGATE_FOR_AGENTS.md](SPECGATE_FOR_AGENTS.md) as the stable handoff doc for agents helping users install or integrate Specgate.

See [Roadmap](docs/roadmap.md) for current closeout status and [archived implementation plan](docs/archive/plans/implementation-plan-v1.1.md#15-post-mvp-work-prioritized) for historical planning context.

## Development

```bash
# Run all tests
cargo test

# Run contract fixtures
cargo test contract_fixtures

# Run Tier A gate
cargo test tier_a_golden

# Run golden corpus
cargo test golden_corpus
```

## License

MIT License. See [LICENSE](LICENSE).
