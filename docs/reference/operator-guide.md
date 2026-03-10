# Specgate Operator Guide

**The definitive onboarding path for Specgate operators.**

This guide connects all key concepts: Wave 0 contract, Tier A gates, golden corpus, and the MVP roadmap. Read this to understand how Specgate works and how to use it effectively.

---

## Navigation

| If you want to... | Go to |
|-------------------|-------|
| Get hands-on immediately | [First 15 Minutes](#first-15-minutes) |
| Understand the contract | [Wave 0 Contract](#wave-0-contract) |
| Set up CI | [CI Gate Understanding](#ci-gate-understanding) |
| Understand fixtures | [Tier A vs Golden Corpus](#tier-a-vs-golden-corpus) |
| See roadmap status | [Release Status](#release-status) |
| Prepare dogfood rollout | [Dogfood Docs](#dogfood-docs) |

---

## First 15 Minutes

### Step 1: Install and Initialize (2 min)

```bash
# Preferred: install the published release artifact for your tag
curl -fsSL -o /tmp/specgate-vX.Y.Z.tar.gz \
  https://github.com/treygoff24/specgate/releases/download/vX.Y.Z/specgate-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz

# Fallback: install the published tag from source
cargo install --locked --git https://github.com/treygoff24/specgate --tag vX.Y.Z

# Initialize a project
cd your-project
specgate init
```

This creates:
- `specgate.config.yml` — Project configuration
- `modules/` — Directory for spec files

### Step 2: Create Your First Spec (5 min)

Create `modules/core-api.spec.yml`:

```yaml
version: "2.2"
module: core/api
description: "Core API module - main entry point"
boundaries:
  public_api:
    - src/api/index.ts
    - src/api/routes/*.ts
  allow_imports_from:
    - core/domain
    - shared/utils
  never_imports:
    - infrastructure/db
  enforce_canonical_imports: true
```

Key fields:
- **`version`**: Must be exactly `"2.2"` (strict matching)
- **`module`**: Unique identifier for this module
- **`public_api`**: Glob patterns for files other modules can import
- **`allow_imports_from`**: Whitelist of importable modules
- **`never_imports`**: Blacklist (deny wins over allow)

### Step 3: Run Your First Check (3 min)

```bash
# Basic check
specgate check

# With diff output
specgate check --baseline-diff

# Only new violations
specgate check --baseline-diff --baseline-new-only
```

### Step 4: Understand the Output (5 min)

The verdict JSON:

```json
{
  "schema_version": "2.2",
  "status": "fail",
  "summary": {
    "total_violations": 1,
    "new_violations": 1,
    "baseline_violations": 0,
    "suppressed_violations": 0,
    "error_violations": 1,
    "warning_violations": 0,
    "new_error_violations": 1,
    "new_warning_violations": 0,
    "stale_baseline_entries": 0
  },
  "violations": [
    {
      "rule": "boundary.allow_imports_from",
      "from_module": "core/api",
      "to_module": "infrastructure/db",
      "from_file": "src/api/handlers/user.ts",
      "severity": "error",
      "disposition": "new",
      "message": "Module `core/api` is not allowed to import from `infrastructure/db` by constraints",
      "fingerprint": "sha256:..."
    }
  ]
}
```

Exit codes:
- `0` — Pass (no new violations)
- `1` — Policy violation (new errors)
- `2` — Runtime/config error

Note: warning-only violations do not fail the pipeline because exit `1` is reserved for new `error`-severity policy violations.
---

## Wave 0 Contract

The [Wave 0 Contract](../archive/status/WAVE0_CONTRACT.md) defines locked semantics that won't change without explicit migration.

### Version Contract

**Supported versions are `2.2` and `2.3`.** This ensures:
- Explicit version updates when specs change
- Unambiguous compatibility
- A stable upgrade path while `2.3` carries contract-boundary features

```yaml
# ✅ Correct
version: "2.2"
version: "2.3"

# ❌ Rejected with clear error
version: "2"
version: "2.0"
```

Use `version: "2.3"` when you need `boundaries.contracts`; `2.2` remains valid for legacy specs without contracts.

### CLI Semantics

#### Baseline Diff Mode

| Flag | Status | Description |
|------|--------|-------------|
| `--baseline-diff` | ✅ Preferred | Output diff between current and baseline |
| `--baseline-new-only` | ✅ Preferred | Show only new violations |
| `--diff` | ⚠️ Deprecated | Alias for `--baseline-diff` (warning emitted) |
| `--diff-new-only` | ⚠️ Deprecated | Alias for `--baseline-new-only` |

#### Git Blast-Radius Mode

Only check modules affected by changes, plus their transitive importers:

```bash
# Full check is safe when the repo/rebase base ref is not available.
specgate check

# Use explicit refs for diff checks.
git fetch origin main --depth=1
specgate check --since origin/main

# Avoid:
# - --since HEAD~1 when there is no previous commit
# - --since main when the local branch is not tracked
```

Blast radius computation:
1. `git diff --name-only --diff-filter=ACMRT <ref>`
2. Map changed files (and changed spec files) to modules
3. Compute transitive importers
4. Filter violations to files in the blast radius

### Rule Precedence

For cross-module edge A → B:

1. `A.never_imports` → **deny**
2. `B.deny_imported_by` → **deny**
3. `B.visibility` → **gate** (private/internal/public + friends)
4. `B.allow_imported_by` → **allowlist**
5. `A.allow_imports_from` → **allowlist**
6. `A.allow_type_imports_from` → **type-only exemption**

**Deny wins over allow at every step.**

---

## CI Gate Understanding

- Start with [MVP Merge Gate](mvp-merge-gate.md) for the canonical merge-ready checklist.
- Use [CI-GATE-UNDERSTANDING.md](../design/ci-gate-understanding.md) for detailed pipeline patterns.

### Quick CI Pattern

```yaml
# .github/workflows/specgate.yml
name: Specgate
on: [push, pull_request]

jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0  # Required for blast-radius
      
      - name: Install Specgate
        run: cargo install --locked --git https://github.com/treygoff24/specgate --tag vX.Y.Z
      
      - name: Check (Full on main, Blast-radius on PRs)
        run: |
          if [ "${{ github.ref }}" == "refs/heads/main" ]; then
            specgate check --output-mode deterministic | tee .specgate-verdict.json
          else
            specgate check --since origin/main --output-mode deterministic | tee .specgate-verdict.json
          fi
      
      - name: Upload Specgate verdict artifact
        uses: actions/upload-artifact@v4
        with:
          name: specgate-verdict
          path: .specgate-verdict.json
```

The workflow above does **not** auto-update baselines. Run baseline regeneration in
manual maintenance windows only:

```bash
specgate baseline --output .specgate-baseline.json
```

### Exit Code Handling

| Code | Meaning | CI Action |
|------|---------|-----------|
| 0 | Pass | Continue |
| 1 | Policy violation | Fail build |
| 2 | Runtime error | Fail with investigation needed |

---

## Tier A vs Golden Corpus

### Tier A Fixtures (The Merge Gate)

**Location:** `tests/fixtures/golden/tier-a/`

Tier A fixtures are:
- **Deterministic** — Byte-identical output across runs
- **Exact-contract** — Not "contains" matching, exact expected violations
- **CI-gating** — Must pass for merge

Gating vs informational:
- **Gating (current):** the enforced merge gate is the exact `scripts/ci/mvp_gate.sh` sequence documented in [MVP Merge Gate](mvp-merge-gate.md), including formatting, clippy, library tests, contract validation/regression suites, `golden_corpus_gate`, `tier_a_golden`, `integration`, `wave2c_cli_integration`, `mvp_gate_baseline`, `doctor_parity_fixtures`, `tsjs_barrel_fixtures`, `tsjs_openclaw_regression`, and `monorepo_integration`.
- **Informational:** `golden_corpus` and ad-hoc fixture experiments remain coverage signals, not enforced merge blockers.

**P0 Fixtures:**

| ID | Rule | What It Tests |
|----|------|---------------|
| A01 | `boundary.allow_imports_from` | Ingress bypassing to infra layer |
| A02 | `boundary.public_api` | Internal file API leak |
| A03 | `enforce-layer` | Layer reversal origin guard |
| A04 | `boundary.canonical_import` | Registry canonical entrypoint |
| A06 | `no-circular-deps` | External cycle detection |

See [Tier A Fixture Design](../design/tier-a-fixture-design.md) for the full specification.

### Golden Corpus (The Safety Net)

**Location:** `tests/fixtures/golden/c01-*`, `c02-*`, etc.

Golden corpus fixtures are:
- **Informational:** `tests/golden_corpus.rs` tracks future-proxy coverage and is not enforced by merge gate.
- **Broader coverage** — More failure classes
- **Tier B** — Candidates for Tier A promotion

Mapping:
- `C02 → A01` (ingress-persistence)
- `C09 → A02` (api-leakage)
- `C08 → A03` (layer-reversal)
- `C07 → A04` (registry-canonical)
- `C06` — deferred from current Tier A scope; deterministic fixtures still missing

### Explicit limitation note (future rule families)

- `C02`: pattern-aware (`no-pattern`) variants are deferred.
- `C06`: category-level governance variants remain informational coverage and are not in the enforced merge gate.
- `C07`: unique-export/visibility edge-case variants are deferred.

---

## Release Status

**Current: release-closeout implementation is landed on `master`; remaining work is release publication/promotion plus deferred backlog beyond this release.**

Current command surface summary:

- `specgate policy-diff` is the implemented governance command for CI policy widening checks.
- `specgate check --format sarif` is available and produces SARIF output.
- `specgate doctor ownership` is available, and `strict_ownership: true` enables CI gating on ownership findings.
- `check --deny-widenings` is implemented for in-band governance checks when `--since <base-ref>` is provided.
- Roadmap source: [docs/roadmap.md](../roadmap.md).

### Completed ✅

| Milestone | Commit | Description |
|-----------|--------|-------------|
| Wave 0 contract | `aa918ad` | CLI semantics, version policy locked |
| Golden v1 scaffold | `2e52949` | Top-5 golden corpus fixtures |
| Tier A P0 | `0297381` | Deterministic gate implemented |
| Reviewer hardening | `7a7fab8` | Near-miss contracts, null handling |
| Merge-gate docs consolidation | `126bc38` / `502ad8a` | Merge-gate and operator docs aligned |

### Release-Closeout Focus 🔄

1. **Adoption CI wiring** — Ensure consumer repos apply the canonical merge-gate contract and failure mapping.
2. **Golden expansion** — Broaden informational corpus coverage for deferred and future rule families.
3. **Dogfood rollout alignment** — Keep CI guidance aligned with the current example workflow and policy-diff+SARIF checks.
4. **Governance follow-through** — Keep CI governance singular (`policy-diff` or `check --deny-widenings`) and aligned across rollout docs.

### Dogfood Docs

- [BASELINE_POLICY](../design/baseline-policy.md) — Baseline update and stale-entry policy.
- [DOGFOOD_ROLLOUT_CHECKLIST](../dogfood/rollout-checklist.md) — Rollout readiness.
- [DOGFOOD_SUCCESS_METRICS](../dogfood/success-metrics.md) — Adoption and reliability targets.
- [DOGFOOD_RELEASE_CHANNEL](../dogfood/release-channel.md) — Stable/beta channel rules.
- [Consumer GitHub Actions workflow](../examples/specgate-consumer-github-actions.yml) — Copy-paste CI integration.
- [Releasing Guide](../../RELEASING.md) — How to ship and promote releases.

See [Roadmap](../roadmap.md#remaining-to-call-this-release-complete) for details.

---

## Key Learnings (Section 16)

From the implementation plan's "Learned During Build":

1. **Precision requires explicit near-miss fixtures** — Not just fail/pass
2. **Null semantics must be contract-tested** — Explicit `to_module: null`, not omitted
3. **Determinism gates need harness-level strictness** — Not just ad-hoc checks
4. **Semantic docs + tests co-evolve** — Wave 0 succeeded because docs and tests merged together
5. **Git-aware blast-radius is high-leverage** — Moves from static checker to practical CI gate

---

## Next Steps

1. **Hands-on:** Complete the [First 15 Minutes](getting-started.md)
2. **Policy governance:** Add `policy-diff` to CI with the canonical example workflow
3. **CI polish:** Verify SARIF and ownership checks in [CI Gate Understanding](../design/ci-gate-understanding.md) and [DOGFOOD_ROLLOUT_CHECKLIST](../dogfood/rollout-checklist.md)
4. **Deep Dive:** Read the [Roadmap](../roadmap.md) and keep [WAVE0_CONTRACT.md](../archive/status/WAVE0_CONTRACT.md) as a source of truth
