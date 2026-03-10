# Tier 2 + Tier 3 Buildout Design

**Date:** 2026-03-10
**Scope:** All deferred product backlog (Tier 2) and historical backlog (Tier 3) items
**Source:** `docs/handoffs/2026-03-10-remaining-buildout-handoff.md`

---

## 1. Cross-File Compensation in `policy-diff`

### Problem

A widening in `auth.spec.yml` paired with a narrowing in `api.spec.yml` currently reports as a widening (exit 1), even when the net policy change is safe.

### Approach

Scoped compensation — only allow compensation between specs that share a dependency relationship in the module graph.

### Design

The `build_policy_diff_report` pipeline gains a new phase after per-file classification:

```
per-file classification (existing)
  → build module graph for changed specs
  → identify compensation candidates (connected widenings + narrowings)
  → apply compensation rules
  → emit CompensatedFieldChange entries
  → final report with net classification
```

**Compensation rules:**

- A narrowing in module A can offset a widening in module B only if A imports from B or B imports from A (direct dependency edge).
- Compensation is field-type-scoped: a `public_api` narrowing can offset a `public_api` widening, but not an `allow_imports_from` widening. Same field family only.
- When compensation is ambiguous (e.g., one narrowing could offset two widenings), fail closed — report as widening.
- Compensation is opt-in: `policy-diff --cross-file-compensation` flag, off by default for backwards compatibility.

**New types:**

- `CompensationCandidate { widening: FieldChange, narrowing: FieldChange, relationship: DependencyEdge }`
- `CompensationResult::Offset | Partial | Ambiguous`
- `PolicyDiffReport` gains `compensations: Vec<CompensationCandidate>` and `net_classification: Classification`

**Output:** Both human and JSON formats show compensated pairs explicitly — never silently swallow a widening. The report says "widening in auth/public_api offset by narrowing in api/public_api (direct dependency)" so reviewers can verify.

**Exit code:** Based on `net_classification` when `--cross-file-compensation` is active, otherwise unchanged.

**Key files:**

- `src/policy/compensate.rs` (new) — compensation logic
- `src/policy/classify.rs` — expose per-file results for compensation input
- `src/policy/mod.rs` — wire compensation phase into pipeline
- `src/policy/types.rs` — new types
- `src/policy/render.rs` — render compensated pairs
- `tests/policy_diff_integration.rs` — compensation test cases

---

## 2. Config-Level Governance Diffing

### Problem

Changes to `specgate.config.yml` can silently weaken enforcement (e.g., adding exclude patterns, relaxing `jest_mock_mode`) and `policy-diff` doesn't see them.

### Approach

Extend `policy-diff` to diff config files using the same classification framework as spec files.

### Design

The discovery phase in `git.rs` gains config awareness:

```
discover_spec_file_changes (existing)
  → also discover_config_file_changes (new)
  → load base/head config blobs via git cat-file
  → deserialize both into SpecConfig
  → classify_config_changes
  → emit ConfigFieldChange entries into PolicyDiffReport
```

**Field classification:**

| Category | Widening | Narrowing |
|----------|----------|-----------|
| `exclude` | added pattern | removed pattern |
| `spec_dirs` | removed dir | added dir |
| `escape_hatches.max_new_per_diff` | increased/removed | decreased/added |
| `escape_hatches.require_expiry` | true→false | false→true |
| `jest_mock_mode` | enforce→warn | warn→enforce |
| `stale_baseline` | fail→warn | warn→fail |
| `enforce_type_only_imports` | true→false | false→true |
| `unresolved_edge_policy` | error→warn→ignore | ignore→warn→error |
| `strict_ownership` | true→false | false→true |
| `import_hygiene.deny_deep_imports` | removed entry | added entry |
| `envelope.enabled` | true→false | false→true |

Everything else (`telemetry`, `release_channel`, `tsconfig_filename`, `test_patterns`, `include_dirs`) is Structural.

**New types:**

- `ConfigFieldChange { field_path: String, classification: Classification, before: String, after: String }`
- `PolicyDiffReport` gains `config_changes: Vec<ConfigFieldChange>`

**Exit code interaction:** Config widenings contribute to the overall report classification. A config widening alone is enough to produce exit 1.

**Edge cases:**

- Config file added where none existed → all non-default fields are Structural (new project, not a weakening)
- Config file deleted → Widening (falling back to defaults may relax enforcement)
- Config file unchanged → no entries emitted

**Key files:**

- `src/policy/config_diff.rs` (new) — config classification logic
- `src/policy/git.rs` — config blob discovery and loading
- `src/policy/types.rs` — `ConfigFieldChange`
- `src/policy/mod.rs` — wire into pipeline
- `src/policy/render.rs` — render config changes
- `tests/policy_diff_integration.rs` — config diff test cases

---

## 3. Rule-Family Fixture Expansion (C02/C06/C07)

### Problem

Several rule scenarios have fixture directories but lack deterministic expected outcomes and aren't merge-gating in Tier A.

### Design

**C02 — Pattern-Aware Mass Assignment**

- New rule: `boundary.pattern_violation` — fires when an import matches a `public_api` glob but the consuming code accesses internals not covered by the contract's `match.pattern`
- Builds on the existing contract system — a contract with `match.pattern` already exists in the schema but isn't enforced as a rule
- Fixture: provider module with a contract specifying `pattern: "^get"`, consumer importing and calling `setPassword()` → violation
- Expected outcome: `boundary.pattern_violation` with severity `error`

**C06 — Category-Level Governance**

- No new engine rule — this is a `doctor` diagnostic
- New `doctor` check: `doctor governance-consistency` — scans modules sharing a namespace prefix and flags contradictory policy fields
- Fixture: `services/auth` is `private`, `services/gateway` has `allow_imports_from: [services/auth]` → info finding about intent mismatch
- Graduates to Tier A as a golden fixture with deterministic output

**C07 — Unique-Export / Visibility Edge Cases**

- New rule: `boundary.visibility_leak` — fires when module A's `public_api` re-exports symbols from module B where B has stricter visibility than A
- Requires the resolver to trace re-export chains (barrel files)
- Fixture: `internal` module re-exports from `private` module's non-public-api file → violation
- Expected outcome: `boundary.visibility_leak` with severity `warning` (configurable)

**Tier A graduation for all three:**

- Each fixture gets an `expected.json` with deterministic violation output
- `tests/tier_a_golden.rs` expanded to include C02, C06, C07
- CI gates on exact match

**Key files:**

- `src/rules/boundary.rs` — `boundary.pattern_violation`, `boundary.visibility_leak`
- `src/cli/doctor/governance.rs` (new) — `doctor governance-consistency`
- `tests/fixtures/golden/c02-mass-assignment/` — updated with pattern contract
- `tests/fixtures/golden/c06-duplicate-key/` — governance consistency scenario
- `tests/fixtures/golden/c07-registry-collision/` — visibility leak scenario
- `tests/tier_a_golden.rs` — new test entries
- `tests/golden_corpus.rs` — updated

---

## 4. Contradictory Glob Detection in Ownership

### Problem

`doctor ownership` catches overlaps and unclaimed files but doesn't detect structural, subset/superset, or semantic contradictions in glob patterns.

### Design

Three tiers of analysis layered into `doctor ownership`.

**Tier 1 — Structural Analysis (Errors)**

Static analysis of glob patterns in isolation:

- Tautological globs — patterns that match everything or nothing
- Negation conflicts — logically impossible patterns
- Duplicate globs — two specs with identical `boundaries.path` values

Implementation: pure glob-string analysis, no filesystem traversal. Runs first as a fast pre-check.

**Tier 2 — Subset/Superset Detection (Warnings)**

Determines containment relationships between glob patterns:

- Strict subset — child spec is fully contained within parent spec, all matched files are shadowed
- Strict superset — a broad spec swallows a narrow one
- Partial overlap with dominance — one glob matches 90%+ of the other's files

Implementation: two-pass approach:
1. Structural containment analysis on glob patterns (fast, no I/O)
2. Empirical analysis against discovered source files when structural analysis is inconclusive

Output: `"src/api/orders/**/*.ts" (module: orders) is a strict subset of "src/api/**/*.ts" (module: api) — all 12 matched files are also claimed by api"`

**Tier 3 — Semantic Conflict Detection (Warnings)**

Cross-references ownership globs against policy fields:

- **Private module referenced as dependency** — module A has `visibility: private` but module B lists A in `allow_imports_from`
- **Denied but friended** — module A has `deny_imported_by: [B]` but also `friend_modules: [B]`
- **Unreachable allow** — module A's `allow_imports_from: [B]` but B has `visibility: private` and A is not in B's `friend_modules`
- **Circular deny** — A denies B and B denies A
- **Ownership gap with contract** — a contract's `match.files` glob references paths outside the module's `boundaries.path`

**Integration with `strict_ownership`:**

- Tier 1 (errors) always fails with `strict_ownership: true`
- Tier 2 and 3 (warnings) fail with `strict_ownership: true` only if `strict_ownership_level: "warnings"` is set (default only gates on errors)

**Key files:**

- `src/spec/ownership.rs` — all three tiers added to `validate_ownership`
- `src/spec/glob_analysis.rs` (new) — structural and subset/superset glob analysis
- `src/spec/semantic_conflicts.rs` (new) — semantic conflict detection logic
- `src/cli/doctor/ownership.rs` — render new finding types
- `tests/ownership_integration.rs` (new) — test cases for each tier

---

## 5. Provider-Side Visibility Model — Gap Completion

### Problem

The foundation exists (`visibility`, `allow_imported_by`, `deny_imported_by`, `friend_modules`) but gaps remain in interaction semantics, namespace inference, transitive enforcement, and glob support.

### Gaps to Fill

**Gap 1 — Namespace inference for `internal` visibility**

- Root-level modules with `internal` visibility: treated as "no valid internal consumers" (effectively `private`) unless `friend_modules` is set
- Namespace matching is strict prefix on `/`-delimited segments, not string prefix
- `services/auth` and `services/gateway` share namespace `services/`; `services/auth` and `services-v2/auth` do not

**Gap 2 — Transitive visibility enforcement**

- When a visibility violation is detected through a re-export chain, the violation detail includes the full chain: `"C → B (re-export) → A, but B is internal to services/"`
- Enriches existing violations, not a new rule

**Gap 3 — `allow_imported_by` and `friend_modules` interaction semantics**

- Both lists grant access independently (union semantics)
- Document explicitly, add test cases confirming union semantics
- Add a `doctor` finding if both lists contain the same module (redundant)

**Gap 4 — Wildcard/glob support in provider-side lists**

- `allow_imported_by`, `deny_imported_by`, and `friend_modules` gain glob matching
- `services/*` matches `services/auth`, `services/gateway` but not `services/deep/nested`
- `services/**` matches all depths
- Biggest functional addition in the visibility model

**Gap 5 — Visibility in verdict output**

- Include `provider_visibility` and `access_grant_reason` (e.g., `"friend_module"`, `"allow_imported_by"`, `"same_namespace"`) in violation entries

**Key files:**

- `src/rules/boundary.rs` — glob matching for provider-side lists, chain detail in violations
- `src/spec/types.rs` — glob parsing for `allow_imported_by`/`deny_imported_by`/`friend_modules`
- `src/verdict/mod.rs` — visibility metadata in violation entries
- `tests/fixtures/golden/tier-a/` — new fixtures for namespace edge cases, transitive chains, glob patterns
- `docs/reference/operator-guide.md` — explicit interaction semantics documented

---

## 6. Unknown Edge Classification (P6)

### Problem

The verdict output doesn't expose edge resolution status. Unresolved imports are either silently ignored or produce a generic warning.

### Design

**Edge taxonomy:**

| Type | Meaning | Example |
|------|---------|---------|
| `resolved` | Mapped to a module | `import { foo } from '../core/utils'` |
| `unresolved_literal` | Static import, couldn't resolve | `import { foo } from './doesNotExist'` |
| `unresolved_dynamic` | Computed import, inherently unresolvable | `import(getModulePath())` |
| `external` | Third-party package | `import lodash from 'lodash'` |

**Verdict changes:**

Every edge in the dependency graph gets tagged. Verdict JSON gains:

```json
{
  "edges": [
    { "from": "api", "to": "core", "type": "resolved", "import_path": "../core/utils" },
    { "from": "api", "to": null, "type": "unresolved_literal", "import_path": "./missing" }
  ],
  "edge_summary": {
    "resolved": 142,
    "unresolved_literal": 3,
    "unresolved_dynamic": 1,
    "external": 28
  }
}
```

**Finding generation:**

Unresolved edges generate findings based on `unresolved_edge_policy` (error/warn/ignore). Rule ID: `hygiene.unresolved_import`.

**SARIF integration:**

Unresolved edge findings emit as SARIF results with `ruleId`, location pointing to the import statement, and `properties.edgeType`.

**No policy-diff interaction** — edge counts are runtime data, not governance state. Config changes to `unresolved_edge_policy` are caught by config-level diffing (Section 2).

**Key files:**

- `src/graph/mod.rs` — tag edges during graph construction
- `src/graph/types.rs` — `EdgeType` enum
- `src/verdict/mod.rs` — edge summary and per-edge type in output
- `src/rules/hygiene.rs` — `hygiene.unresolved_import` rule
- `src/cli/mod.rs` — SARIF emission for edge findings
- `tests/edge_classification_integration.rs` (new)

---

## 7. Baseline v2 Metadata

### Problem

Baseline entries are opaque — no `owner`, `reason`, or `added_at` metadata. The `expires_at` field exists but the rest does not.

### Design

**Extended baseline entry format:**

```yaml
- rule: boundary.allow_imports_from
  module: api/orders
  detail: "imports from persistence/internal"
  fingerprint: "a1b2c3d4"
  owner: "team-payments"
  reason: "legacy migration — removing after Q3 refactor"
  expires_at: "2026-06-30"
  added_at: "2026-03-10"
```

All new fields are optional for backwards compatibility.

**CLI changes:**

`specgate baseline add` gains `--owner`, `--reason` flags. `--added-at` is auto-populated.

New config field: `baseline.require_metadata: bool` (default `false`) — when true, `baseline add` without `--owner` and `--reason` fails.

`specgate baseline list` gains filtering:

```
specgate baseline list --owner "team-payments"
specgate baseline list --expired
specgate baseline list --expiring-within 30
specgate baseline list --group-by owner
specgate baseline list --group-by rule
specgate baseline list --format json
```

**New subcommand: `specgate baseline audit`**

Summarizes baseline health:

```
Baseline Health: 23 entries

  By owner:
    team-payments     8 entries (2 expired)
    team-platform     6 entries (0 expired)
    <no owner>        9 entries

  Expiry status:
    Expired           3
    Expiring < 30d    5
    No expiry set     7
    Active            8

  Metadata coverage:
    Has owner         14/23 (61%)
    Has reason        11/23 (48%)
```

**Integration with governance:**

When `baseline.require_metadata: true`:
- `baseline add` without `--owner`/`--reason` → error
- `baseline audit` with metadata gaps → non-zero exit code

**Key files:**

- `src/baseline/mod.rs` — extended entry struct, parsing
- `src/baseline/audit.rs` (new) — audit logic and reporting
- `src/cli/baseline.rs` or `src/cli/mod.rs` — flags, filtering, audit subcommand
- `src/spec/config.rs` — `baseline.require_metadata` config field
- `tests/baseline_integration.rs` — metadata round-trip, filtering, audit

---

## 8. Import Hygiene Rules (P9)

### Problem

No enforcement for deep third-party imports, test-production boundary violations, or granular canonical import control.

### Design

Three rule families. Config sets defaults, per-module spec overrides, spec wins.

**Rule 1: `hygiene.deep_third_party_import`**

Config layer:

```yaml
import_hygiene:
  deny_deep_imports:
    - pattern: "lodash/**"
      max_depth: 1
    - pattern: "@mui/**"
      max_depth: 2
    - pattern: "*"
      max_depth: 2
```

Spec override layer:

```yaml
boundaries:
  import_hygiene:
    deny_deep_imports:
      - pattern: "lodash/**"
        max_depth: 0
      - pattern: "internal-sdk/**"
        allow: true
```

Merge semantics: spec entries override config entries for matching patterns. Unmatched patterns fall through to config defaults.

**Rule 2: `hygiene.test_in_production`**

Config layer:

```yaml
import_hygiene:
  test_boundary:
    enabled: true
    mode: "bidirectional"    # or "production_only"
```

Spec override:

```yaml
boundaries:
  import_hygiene:
    test_boundary:
      mode: "off"
```

Detection:
- Production file importing a path matching `test_patterns` → violation
- Test file importing a non-`public_api` path from another module → violation (bidirectional mode only)
- Test file importing from its own module's internals → allowed

**Rule 3: `boundary.canonical_import_dangling`**

- If `import_id` or `import_ids` point to paths not covered by `public_api` globs, emit as a `doctor` finding
- No new spec field needed — `enforce_canonical_imports` already exists per-module

**Severity defaults:**

| Rule | Default | Configurable |
|------|---------|--------------|
| `hygiene.deep_third_party_import` | `warning` | Yes, per config pattern |
| `hygiene.test_in_production` | `error` | Yes, per config |
| `boundary.canonical_import_dangling` | `warning` | No (doctor finding) |

**SARIF integration:** All three rules emit SARIF results with file/line location.

**Key files:**

- `src/rules/hygiene.rs` — deep import and test boundary enforcement
- `src/rules/boundary.rs` — canonical import dangling check
- `src/spec/config.rs` — extended `ImportHygieneConfig`
- `src/spec/types.rs` — per-module `import_hygiene` on `Boundaries`
- `src/cli/doctor/` — canonical import dangling as doctor check
- `tests/hygiene_integration.rs` (new)
- `tests/fixtures/` — fixtures exercising each rule

---

## Verification Baseline

All work must pass:

```bash
cargo test
cargo check --all-targets
cargo clippy --all-targets -- -D warnings
cargo fmt --check
./scripts/ci/mvp_gate.sh
```

## Recommended Build Order

1. **Cross-file compensation** — core governance gap, high value
2. **Config-level governance diffing** — pairs naturally with compensation work
3. **Unknown edge classification** — independent, enriches verdict output
4. **Baseline v2 metadata** — independent, enriches baseline workflow
5. **Import hygiene rules** — builds on config and spec type changes from earlier
6. **Provider-side visibility gaps** — builds on rule engine changes from hygiene
7. **Contradictory glob detection** — builds on visibility model completion
8. **Rule-family fixture expansion** — last, depends on all engine features being stable
