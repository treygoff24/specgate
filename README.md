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
| [Archive Index](docs/archive/ARCHIVE_INDEX.md) | Superseded plans, reviews, and release artifacts |

## Install options

- Preferred path: download the release tarball + `.sha256` for your tag (example `v0.1.0-rc3`) and run the checksum check before using `specgate`.
- Fallback path: `cargo install --locked --git https://github.com/treygoff24/specgate --tag v0.1.0-rc3`.
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

## Project Status

**Status (as of 2026-02-28): MVP implementation and merge gates are complete and passing on `master`; active work is post-MVP release/adoption follow-through.**

### Completed
- ✅ Wave 0 contract lock (CLI semantics, version policy)
- ✅ Golden corpus scaffold (top-5 fixtures)
- ✅ Tier A P0 fixtures (deterministic CI gate)
- ✅ Baseline fingerprinting and blast-radius mode
- ✅ Merge gate command contract and operator runbook alignment

### Post-MVP Follow-Through
- 📌 Explicitly deferred rule families (pattern-aware `C02`, governance-only `C06`, friend-surface `C07`) remain out-of-scope for MVP and are tracked as roadmap work.
- 📌 Golden corpus expansion continues as non-blocking coverage growth.
- 📌 Governance readability and review ergonomics remain active operator UX improvements.

See [Implementation Plan](docs/specgate-implementation-plan-v1.1.md#15-post-mvp-work-prioritized) for details.

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
