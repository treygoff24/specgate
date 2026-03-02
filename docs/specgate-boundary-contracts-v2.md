# Specgate Boundary Contracts — Spec V2

*2026-02-27. Updated 2026-03-02 with dogfooding learnings from Hearth integration.*
*Synthesized from V1 draft + Vulcan xhigh (implementation review) + Athena (architecture review) + Paul Bohm's framing.*

## Origin

Paul Bohm's thesis: when AI agents generate code at scale, the bottleneck shifts from writing to verification. The winning pattern is contract-driven boundary enforcement that makes entire classes of bugs structurally impossible. Specgate already embodies most of this at the specification layer. This extension closes the gap between "module boundaries are declared" and "data crossing those boundaries is validated."

## Dogfooding Context (2026-03-02)

Specgate was dogfooded on the Hearth codebase (React + Node.js, ~156 source files, 386 import edges). Key learnings that inform this spec:

1. **Zero-violation baseline is achievable.** A well-structured codebase passes on first run. The prescriptive approach (define ideal architecture, then enforce) works better than the descriptive approach (baseline current state).
2. **Glob semantics matter.** Found and fixed a `literal_separator` bug where `*` matched across directory separators. Any new glob-based matching (like `match.files`) must use `literal_separator(true)`.
3. **Nested module ownership needs care.** Parent modules (`server`) and child modules (`server/bridge`) can overlap. The `path` glob must be precise. Contract `match.files` patterns will face the same issue.
4. **CI integration is straightforward.** Blast-radius mode (`--since origin/main`) works well for PR checks. Contract violations should integrate seamlessly with existing `--since` filtering.
5. **The tool is fast enough.** 325ms for 156 files / 386 edges. Contract file existence checks add negligible overhead. AST-based envelope checks (Phase 5) need to be similarly fast.

## Design Principles

1. **Transport-agnostic contracts.** Specgate doesn't care *how* data crosses a boundary, only *that* it crosses through a validated choke point. The contract is a refinement type: `Unvalidated<T> → Validated<T>`.

2. **Unify with existing boundary primitives.** Contracts attach to existing `boundaries` declarations. A `public_api` export can require a contract. An `allow_imports_from` edge can require contract validation. No new top-level concept needed.

3. **Deterministic enforcement only in CI.** Heuristic scanning stays out of `specgate check`. Heuristics exist only in a local `specgate discover` command for bootstrapping. The CI gate remains 100% deterministic and AST-driven.

4. **Specgate proves wiring, not correctness.** Specgate can prove that a contract exists, is referenced, and that code calls the validator. It cannot prove the contract declares the right fields or that business logic is correct. This is a feature, not a limitation.

5. **Code-site binding required for coverage claims.** `match` selector makes coverage deterministic, not hand-wavy.

6. **Glob patterns use `literal_separator(true)`.** All new glob matching must use `GlobBuilder` with `literal_separator(true)` to prevent `*` from matching across directory separators. This was a real bug found during Hearth dogfooding (fixed in `0f9dc81`).

## Spec Language Extension

### Version: 2.3 (with backward compatibility)

**Implementation requirements:**
- Change `SUPPORTED_SPEC_VERSION` in `src/spec/types.rs` from a single `&str` to an accepted-versions set: `["2.2", "2.3"]`
- Version validation in `src/spec/validation.rs` must accept both
- `contracts` field is only valid in `2.3` specs — if present in a `2.2` spec, emit a validation error with hint to upgrade version
- `deny_unknown_fields` on `Boundaries` struct must be relaxed to allow `contracts` field when version is `2.3` (or: add `contracts` to the struct with `#[serde(default)]` and validate version-gating in the validation pass, not at parse time)
- Existing `2.2` specs continue to parse and validate unchanged

**Recommended approach:** Add `contracts` to `Boundaries` struct with `#[serde(default)]` and a `skip_serializing_if` empty check. Validate in `src/spec/validation.rs` that `contracts` is empty when version is `"2.2"`. This avoids conditional deserialization complexity.

### Schema: Contract-Augmented Boundaries

Contracts attach to existing boundary declarations:

```yaml
version: "2.3"
module: api/handlers

boundaries:
  allow_imports_from:
    - core/domain
    - shared/utils

  public_api:
    - src/api/handlers/index.ts
    - src/api/handlers/users.ts

  contracts:
    - id: "create_user"
      contract: "contracts/create-user.json"    # path relative to project root
      match:
        files: ["src/api/handlers/users.ts"]
        pattern: "createUser"                    # symbol/function name (AST-matched)
      direction: inbound                         # inbound | outbound | bidirectional
      envelope: optional                         # optional | required (default: optional)

    - id: "billing_fetch"
      contract: "contracts/billing.json"
      match:
        files: ["src/api/handlers/billing.ts"]
        pattern: "fetchBillingData"
      direction: outbound
```

### Rust Types (new additions to `src/spec/types.rs`)

```rust
/// A boundary contract declaration (spec version 2.3+).
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BoundaryContract {
    /// Unique identifier within the module.
    pub id: String,
    /// Path to contract file, relative to project root.
    pub contract: String,
    /// Code-site binding.
    pub r#match: ContractMatch,
    /// Data flow direction at this boundary.
    pub direction: ContractDirection,
    /// Envelope validation requirement.
    #[serde(default)]
    pub envelope: EnvelopeRequirement,
    /// Optional cross-module consumer references.
    #[serde(default)]
    pub imports_contract: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ContractMatch {
    /// Glob patterns for files containing this boundary crossing.
    pub files: Vec<String>,
    /// Symbol/function name for AST-level binding (optional).
    #[serde(default)]
    pub pattern: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ContractDirection {
    Inbound,
    Outbound,
    Bidirectional,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EnvelopeRequirement {
    #[default]
    Optional,
    Required,
}
```

Add to `Boundaries` struct:
```rust
// In Boundaries struct, after existing fields:

/// Boundary contracts (spec version 2.3+ only).
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub contracts: Vec<BoundaryContract>,
```

### Field Semantics

| Field | Required | Description |
|-------|----------|-------------|
| `id` | yes | Unique within module. Used in diagnostics and cross-module references. |
| `contract` | yes | Path to contract file, relative to project root. Must exist and be non-empty. |
| `match.files` | yes | Glob patterns for files containing this boundary crossing. Uses `literal_separator(true)`. |
| `match.pattern` | no | Symbol/function name for AST-level binding. If omitted, coverage is file-level only. |
| `direction` | yes | `inbound` (data entering module), `outbound` (data leaving), `bidirectional`. |
| `envelope` | no | `optional` (default) or `required`. When `required`, Specgate checks that code calls an envelope validator. |
| `imports_contract` | no | Cross-module contract references in format `"module_id:contract_id"`. Specgate validates referenced contracts exist. |

### Contract File Format

**Position: format-agnostic enforcement, parseable-format recommendation.**

Specgate enforces:
- File exists at declared path (relative to project root)
- File is non-empty
- File extension is one of: `.json`, `.yaml`, `.yml`, `.ts`, `.zod`, `.proto`

Specgate does NOT parse or validate contract file contents. Future versions may add pluggable schema validators.

### Cross-Module Boundaries

**Single owner, optional consumer reference.**

When a boundary spans two modules (producer in A, consumer in B):
- The **owner module** declares the contract with full `match` binding
- Consumer modules **may** reference it: `imports_contract: ["api/handlers:create_user"]`
- Specgate validates that referenced contracts exist
- Dual declaration is NOT required (prevents drift)

## New Violation Types

| Violation ID | Severity | Trigger | Phase |
|-------------|----------|---------|-------|
| `boundary.contract_missing` | error | Contract declared but file doesn't exist | 2 |
| `boundary.contract_empty` | error | Contract file exists but is empty | 2 |
| `boundary.match_unresolved` | error | `match.files` pattern resolves to zero files | 2 |
| `boundary.contract_ref_invalid` | error | `imports_contract` references a non-existent module or contract ID | 2 |
| `boundary.contract_version_mismatch` | error | `contracts` field present in a `2.2` spec | 1 |
| `boundary.envelope_missing` | warning | `envelope: required` but no envelope validator call found in matched code | 5 |
| `boundary.unmatched_crossing` | warning | Discovered boundary-like pattern not covered by any contract (only from `specgate discover`) | 4 |

**Implementation notes:**
- New rule IDs must be added to `KNOWN_CONSTRAINT_RULES` in `src/spec/validation.rs`
- Wire through `src/rules/mod.rs` → `src/verdict/mod.rs` → CLI pipeline
- The `fix_hint` pattern from `LayerViolation` in `src/rules/layers.rs` should be extended to all new violation types
- All contract violations must include `contract_id` in the violation output for machine-readable diagnostics
- Contract violations must work with `--since` blast-radius mode: only evaluate contracts in modules affected by the diff

## Structured Diagnostics

All violations gain optional structured fields:

```rust
// Additions to VerdictViolation in src/verdict/mod.rs
pub struct VerdictViolation {
    // ... existing fields ...

    /// What was expected (human-readable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    /// What was actually found (human-readable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual: Option<String>,
    /// Actionable fix suggestion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remediation_hint: Option<String>,
    /// Contract ID if this violation relates to a contract.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contract_id: Option<String>,
}
```

**Output format:** `specgate check --format json` emits NDJSON (one violation per line). Default output remains structured JSON. This is additive, not a breaking change.

**Verdict schema versioning:** Decouple verdict output schema version from spec-language version. Currently both use `"2.2"`. Add `verdict_schema: "1.0"` as a separate version track in the verdict output. Spec `schema_version` field continues to track the spec language version used.

## Human-Readable Output

Add `--format human` (and make it the default for interactive terminals, with `--format json` as default for piped output):

```
$ specgate check

  ✗ boundary.contract_missing  api/handlers:create_user
    Contract file not found: contracts/create-user.json
    → Create contracts/create-user.json with the schema for createUser

  ✗ boundary.match_unresolved  api/handlers:billing_fetch
    No files match: src/api/handlers/billing.ts
    → Check that the file exists and the glob pattern is correct

  Summary: 2 errors, 0 warnings (0 baseline, 2 new)
```

Implementation: detect `isatty(stdout)` to choose default format. `--format human|json|ndjson` flag overrides.

## Discovery Tool (NOT CI)

`specgate discover` — local-only command for scanning undeclared boundary crossings.

```bash
specgate discover                    # scan and report
specgate discover --scaffold         # generate contract stubs (dry-run)
specgate discover --scaffold --write # actually create files
```

**Scanner (TS/JS only in V1):**
- High-confidence AST patterns: `fetch()`, `process.env.*`, known SDK imports (axios, prisma, database clients)
- Uses existing `oxc_parser` traversal in `src/parser/mod.rs`
- Confidence scoring: HIGH / MEDIUM / LOW
- Conservative defaults; false positives are acceptable for a local dev tool

## Envelope Protocol

### What Specgate checks (static, Phase 5)

When `envelope: required` on a contract:
1. The matched file imports a function matching `boundary.validate` or a configurable import pattern
2. The boundary-crossing code calls the validator with the contract ID as first argument
3. Both checks are AST-based, deterministic, and cacheable

### What Specgate does NOT do (runtime)
- Validate payloads at runtime
- Generate envelope validators
- Enforce envelope structure

### Configurable envelope pattern

In `specgate.config.yml`:
```yaml
envelope:
  import_pattern: "specgate-envelope"    # package name to look for
  function_pattern: "boundary.validate"  # call expression to match
```

Defaults to the reference implementation's patterns. Teams using custom validators can override.

### Reference envelope library (separate package)

```typescript
// specgate-envelope (npm package, separate repo)
import { boundary } from 'specgate-envelope';

const result = boundary.validate('create_user', 'v1', rawInput);
// Returns { contractId, schemaVersion, producer, scope, payload } or throws
```

## Implementation Phases

### Phase 1: Spec Language + Parsing (2-3 days)

**Rust changes:**
- `src/spec/types.rs`: Add `BoundaryContract`, `ContractMatch`, `ContractDirection`, `EnvelopeRequirement` structs. Add `contracts: Vec<BoundaryContract>` to `Boundaries` with `#[serde(default, skip_serializing_if = "Vec::is_empty")]`.
- `src/spec/types.rs`: Change version handling — replace `SUPPORTED_SPEC_VERSION: &str = "2.2"` with `SUPPORTED_SPEC_VERSIONS: &[&str] = &["2.2", "2.3"]`. Update version check in validation to use `.contains()`.
- `src/spec/validation.rs`: Add validation pass for contracts:
  - If version is `"2.2"` and contracts is non-empty → `boundary.contract_version_mismatch` error
  - Contract `id` uniqueness within module
  - `match.files` patterns are valid globs (use `GlobBuilder` with `literal_separator(true)`)
  - `direction` is a valid enum value
  - `imports_contract` references use valid `"module:id"` format
- `tests/`: Golden corpus fixtures for contract declarations (valid 2.3 spec, invalid 2.2 spec with contracts, duplicate IDs, invalid globs)
- `specgate init`: When generating starter spec, default to version `"2.3"` with an empty contracts array

**Gate:** `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && ./scripts/ci/mvp_gate.sh`

### Phase 2: Contract Enforcement Rules (2-3 days)

**Rust changes:**
- `src/rules/contracts.rs` (new file): Contract-specific rule evaluation
  - `boundary.contract_missing`: Check contract file exists at declared path (project-root-relative)
  - `boundary.contract_empty`: Check contract file is non-empty
  - `boundary.match_unresolved`: Check `match.files` globs resolve to at least one file in the dependency graph
  - `boundary.contract_ref_invalid`: Validate `imports_contract` references resolve to existing modules and contract IDs
- `src/rules/mod.rs`: Wire `contracts.rs` into the rule evaluation pipeline
- `src/verdict/mod.rs`: Add `contract_id: Option<String>` to `VerdictViolation`
- `src/cli/mod.rs`: Integrate contract rules into `analyze_project` flow. Ensure `--since` blast-radius filtering applies (only evaluate contracts in affected modules).
- `src/spec/validation.rs`: Add new rule IDs to `KNOWN_CONSTRAINT_RULES`
- `tests/`: Integration tests for each violation type, including baseline interaction (contract violations should be baselineable)

### Phase 3: Structured Diagnostics (1-2 days)

**Rust changes:**
- `src/verdict/mod.rs`: Add `expected`, `actual`, `remediation_hint` fields to `VerdictViolation` (all `Option<String>`, skip_serializing_if none)
- All existing violation producers: Add hints where useful (especially `boundary.allow_imports_from`, `boundary.never_imports`)
- All contract violations from Phase 2: Populate structured fields
- Surface existing `fix_hint` from `LayerViolation` (currently dropped in `analyze_project`)
- `src/cli/mod.rs`: Add `--format human|json|ndjson` flag. Detect `isatty(stdout)` for default.
- Add human-readable formatter that prints violations with color, hints, and summary line
- Decouple verdict schema version: add `verdict_schema: "1.0"` field to verdict output

### Phase 4: Discovery Tool (2-3 days)

**Rust changes:**
- `src/discover/mod.rs` (new module): Boundary-crossing pattern scanner
- `src/cli/mod.rs`: Add `specgate discover` subcommand
- Scanner uses `oxc_parser` AST traversal to find:
  - `fetch()` / `axios.*` / `got.*` calls (HTTP boundary)
  - `process.env.*` access (environment boundary)
  - Database client calls (prisma, knex, sequelize, pg, mysql patterns)
  - `fs.*` / `path.*` with external paths (filesystem boundary)
- Confidence scoring based on pattern specificity
- `--scaffold` flag: Generate contract stubs + update spec files (dry-run default, `--write` to persist)
- Output: structured list of candidates with file, line, confidence, suggested contract

### Phase 5: Envelope Static Check (2-3 days)

**Rust changes:**
- `src/rules/contracts.rs`: Add envelope validation check
- For each contract with `envelope: required`:
  - Parse matched files (already in dependency graph)
  - Check for import of envelope package (configurable pattern)
  - Check for call expression matching `boundary.validate('contract_id', ...)` or configured pattern
  - AST-level: walk `CallExpression` nodes, check callee matches pattern, check first argument is string literal matching contract ID
- `src/spec/config.rs`: Add `envelope` config section for customizable import/function patterns
- `boundary.envelope_missing` violation when checks fail
- Tests: fixtures with/without envelope calls, custom patterns, edge cases (renamed imports, destructured imports)

### Phase 6: CI Gate Integration (1 day)

- `specgate.config.yml`: Add `boundary_coverage_minimum: 0.0..1.0` config option
- Coverage metric: `(files with contract-matched crossings) / (files with discovered boundary patterns)`
- Requires Phase 4 discovery results as denominator
- `--since` interaction: coverage evaluates blast radius only
- `boundary.unmatched_crossing` is baselineable for incremental adoption
- Add coverage summary to verdict output

## What This Proves (Paul's Framework)

After full implementation, Specgate can mechanically prove:

| Claim | How |
|-------|-----|
| Every module boundary is declared | Spec files + module map |
| Every import respects boundaries | Import-graph analysis + `allow_imports_from` / `never_imports` |
| Every boundary crossing has a contract | Contract declarations + `match` binding |
| Every contract is validated at runtime | Envelope static check |
| No drift between schema and enforcement | Single contract file is source of truth |
| Every error is classified | Structured diagnostics with exhaustive violation taxonomy |

What it deliberately cannot prove:
- The contract itself is semantically correct
- Business logic inside the boundary is correct
- Runtime behavior matches static declarations

This boundary is a feature. Trying to verify business logic correctness leads to formal verification rabbitholes with diminishing returns for agent-scale development.

## Open Items Resolved

| Question | V1 | V2 Resolution |
|----------|-----|---------------|
| Contract file format | Unresolved | Format-agnostic enforcement, parseable-format recommendation |
| Heuristic in CI? | Advisory warnings in `specgate check` | Heuristics banned from CI. Local `specgate discover` only |
| Migration tooling | Unresolved | `specgate discover --scaffold` with dry-run default |
| Cross-module | Unresolved | Single owner + optional consumer reference |
| Envelope timing | Deferred to V2 | In-scope, phased after contract declarations |
| `kind` enum | Transport-specific enum | Removed. Contracts are transport-agnostic |
| Code-site binding | Missing | `match` field with files + pattern selectors |
| Version bump strategy | Unresolved | Accept both 2.2 and 2.3; contracts only valid in 2.3 |
| Glob semantics | Not discussed | `literal_separator(true)` mandatory for all path globs (bug found in dogfooding) |
| Human output | JSON only | `--format human` with isatty detection |

## Appendix: Reviewer Disagreements and Resolutions

### Heuristics in CI
- **Athena:** "Heuristics destroy trust in enforcement tools. Ban them from CI entirely."
- **Vulcan:** "Advisory warnings in CI are fine if conservative."
- **Resolution:** Athena wins. CI must be deterministic. Heuristics live in `specgate discover` only.

### Envelope timing
- **Athena:** "Envelope is THE core insight. Phase 1, not deferred."
- **Vulcan:** "Defer envelope. Focus on declarations first."
- **Resolution:** Compromise. Envelope is in-scope but phased after declarations (Phase 5). The envelope's static check depends on having contract declarations and match bindings first.

### Cross-module contracts
- **Athena:** "Declare in both (session types). Producer offers, consumer accepts."
- **Vulcan:** "Single source of truth. Dual declarations drift."
- **Resolution:** Vulcan wins for V1. Single owner with optional consumer references.

### Contract file format
- **Athena:** "Be opinionated. Pick JSON Schema."
- **Vulcan:** "Format-agnostic. Enforce existence only."
- **Resolution:** Agnostic enforcement, strong recommendation in docs.

### `crossings` as a primitive
- **Athena:** "Wrong abstraction. Conflates transport with data contracts."
- **Vulcan:** "Schema is human-friendly but underspecified."
- **Resolution:** Athena wins on abstraction (removed `kind` enum, transport-agnostic), Vulcan wins on specificity (added `match` field for code-site binding). Renamed to `contracts`.

## Appendix: Dogfooding Learnings (Hearth, 2026-03-02)

**What worked:**
- Prescriptive spec approach (define ideal, enforce) produced zero violations on a well-structured codebase
- 7 specs covering 3 top-level boundaries + 4 server submodules — right granularity
- Blast-radius mode in CI workflow works well for PR checks
- 325ms analysis time is fast enough for CI and local dev

**What broke:**
- `Glob::new()` without `literal_separator(true)` caused `*` to match across directories → module map overlaps → silent enforcement failures. Fixed in `0f9dc81`.
- Parent module (`server` with `src/server/*.ts`) overlapped with child modules (`server/bridge` with `src/server/bridge/**`). Worked around by narrowing parent to `src/server/index.ts`. The glob fix makes this less error-prone but operators still need to think about overlap.

**What's missing (motivates this spec):**
- No way to express "this module's public API requires callers to validate input against a schema"
- No way to verify that webhook/HTTP handlers actually validate payloads
- No way to discover undeclared external boundaries (fetch calls, env access, DB queries)
- The import graph tells you *what talks to what* but not *whether the conversation is safe*
