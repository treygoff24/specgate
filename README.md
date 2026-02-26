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

See [First 15 Minutes Guide](docs/getting-started.md#first-15-minutes) for the full walkthrough.

## Documentation

| Document | Purpose |
|----------|---------|
| [**Operator Guide**](docs/OPERATOR_GUIDE.md) | **Start here** — Complete onboarding path |
| [First 15 Minutes](docs/getting-started.md) | Quick hands-on tutorial |
| [Spec Language Reference](docs/spec-language.md) | YAML spec file format |
| [CI Gate Understanding](docs/CI-GATE-UNDERSTANDING.md) | How Specgate works in CI |
| [Tier A Fixture Design](docs/tier-a-fixture-design-v1.md) | Deterministic CI gate contract |
| [Implementation Plan](docs/specgate-implementation-plan-v1.1.md) | Full MVP roadmap and status |
| [Wave 0 Contract](WAVE0_CONTRACT.md) | Locked CLI semantics and version policy |

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

See [CI Gate Understanding](docs/CI-GATE-UNDERSTANDING.md) for complete CI patterns.

## Project Status

**MVP: ~80% complete** — Core engine and contract-critical semantics are in place.

### Completed
- ✅ Wave 0 contract lock (CLI semantics, version policy)
- ✅ Golden corpus scaffold (top-5 fixtures)
- ✅ Tier A P0 fixtures (deterministic CI gate)
- ✅ Baseline fingerprinting and blast-radius mode

### Remaining
- 🔄 CI wiring and merge gate documentation
- 🔄 Golden corpus expansion
- 🔄 Doctor UX parity tooling
- 🔄 Governance hardening

See [Implementation Plan](docs/specgate-implementation-plan-v1.1.md#15-remaining-work-prioritized) for details.

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

[Specify license here]
