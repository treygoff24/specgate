# Specgate

**File-edge structural policy engine with deterministic output contract.**

Specgate enforces architecture boundaries, layer constraints, and dependency rules for TypeScript/JavaScript projects with byte-identical output for CI reliability.

## Quick Start (First 15 Minutes)

```bash
# 1. Initialize a new project
specgate init

# 2. Create your first spec file
cat > modules/my-module.spec.yml << 'EOF'
version: "2.2"
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

## Documentation

### Reference (start here)

| Document | Purpose |
|----------|---------|
| [**Operator Guide**](docs/reference/operator-guide.md) | **Start here** — Complete onboarding path |
| [First 15 Minutes](docs/reference/getting-started.md) | Quick hands-on tutorial |
| [Spec Language Reference](docs/reference/spec-language.md) | YAML spec file format |
| [Policy diff reference](docs/reference/policy-diff.md) | Compare `.spec.yml` policy across git refs and detect widenings |
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

Use `specgate policy-diff --base origin/main` to compare `.spec.yml` policy between refs and classify the result as widening, narrowing, or structural. Add `--head <ref>` to compare explicit refs and `--format json` or `--format ndjson` for machine readable output. Exit `0` means no widenings were detected, exit `1` means one or more widenings were detected, and exit `2` means the command could not complete because of a git or parse error.

You can enforce the same governance directly in `check` with `--deny-widenings` when `--since` is provided:

```bash
specgate check --since origin/main --deny-widenings
```

With this flag, widening changes force exit `1`, governance/runtime failures force exit `2`, and non-widening diffs keep normal `check` behavior.

Use exactly one governance gate in CI (`policy-diff` or `check --deny-widenings`) so failures are not duplicated.

For exit code `2`, `policy-diff` keeps structured entries in `errors` but clears authoritative classification payload fields (`diffs` and non-zero summary counters) so consumers do not treat partial output as a gate signal.

In the MVP, deleting a `.spec.yml` file is always a widening. Renames/copies use semantic pairing: equivalent snapshots are `structural`, while inconclusive pairings stay fail-closed as widening risk. In CI, fetch full history before diffing against remote refs.

```yaml
- uses: actions/checkout@v4
  with:
    fetch-depth: 0

- name: Detect policy widenings
  run: specgate policy-diff --base origin/main
```

See [Policy diff reference](docs/reference/policy-diff.md) for format details, examples, shallow clone guidance, and current deferred items.

### Upgrade guidance

- Governance: pick one gate path for PRs - `specgate policy-diff --base <ref>` or `specgate check --since <ref> --deny-widenings`.
- SARIF: add `specgate check --format sarif > specgate.sarif` when uploading results to code scanning platforms.
- Ownership diagnostics: add `specgate doctor ownership --project-root . --format json` and enable `strict_ownership: true` when you want findings to fail CI.
- Fetch depth: when diffing against remote refs, use full history (`fetch-depth: 0`).

## Project Status

**Status (as of 2026-03-10): release-closeout implementation is landed on `master`, with Phase 5 envelope checks, policy-diff, `check --deny-widenings`, SARIF output, doctor ownership, monorepo support, adversarial fixtures, and CLI refactor updates all in place. The remaining work is the operational release publish step (tag + notes), tracked in the roadmap.**

### Completed
- ✅ Envelope validation in Phase 5: contract `envelope` rules, scoped function matching, and static boundary checks.
- ✅ `specgate policy-diff` for policy evolution checks, with multiple output formats and clear failure semantics.
- ✅ `specgate check --deny-widenings` for single-command governance enforcement when using `--since`.
- ✅ SARIF reporting via `--format sarif` for CI security scanning workflows.
- ✅ `specgate doctor ownership` for ownership diagnostics and strict CI-friendly enforcement.
- ✅ Full monorepo support including workspace discovery, nearest-tsconfig resolution, and `workspace_packages` reporting.
- ✅ Expanded adversarial and parity fixtures plus coverage in contract/golden/CI test sets.
- ✅ CLI refactor work for modular command structure and stable command-level diagnostics.

### Next Steps
- 📌 Revalidate release gates on the release commit and publish the next tag + release notes.
- 📌 Keep deferred backlog items explicit in the roadmap and operator references.

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
