# Specgate Boundary Contracts — Spec V2

*2026-02-27. Synthesized from V1 draft + Vulcan xhigh (implementation review) + Athena (architecture review) + Paul Bohm's framing.*

## Origin

Paul Bohm's thesis: when AI agents generate code at scale, the bottleneck shifts from writing to verification. The winning pattern is contract-driven boundary enforcement that makes entire classes of bugs structurally impossible. Specgate already embodies most of this at the specification layer. This extension closes the gap between "module boundaries are declared" and "data crossing those boundaries is validated."

## Design Principles (V2 additions from review)

1. **Transport-agnostic contracts.** V1 had a `kind` enum (http_inbound, db_read, sdk_call). Athena correctly identified this as a category error. Specgate should not care *how* data crosses a boundary, only *that* it crosses through a validated choke point. The contract is a refinement type: `Unvalidated<T> → Validated<T>`.

2. **Unify with existing boundary primitives.** V1 introduced `crossings` as a third parallel concept alongside `public_api` and `allow_imports_from`. V2 extends the existing primitives instead. A `public_api` export can require a contract. An `allow_imports_from` edge can require contract validation. No new top-level concept needed.

3. **Deterministic enforcement only in CI.** Heuristic scanning stays out of `specgate check`. Heuristics exist only in a local `specgate discover` command for bootstrapping. The CI gate remains 100% deterministic and AST-driven.

4. **Specgate proves wiring, not correctness.** Specgate can prove that a contract exists, is referenced, and that code calls the validator. It cannot prove the contract declares the right fields or that business logic is correct. This is a feature, not a limitation.

5. **Code-site binding required for coverage claims.** V1 had no mechanism to link a declaration to actual code. V2 adds a `match` selector so coverage is deterministic, not hand-wavy.

## Spec Language Extension

### Version: 2.3 (with backward compatibility)

**Critical implementation note** (from Vulcan): Version handling is strict exact-match (`SUPPORTED_SPEC_VERSION` in `src/spec/types.rs`). A naïve bump breaks all existing `2.2` specs. V2 must:
- Add `2.3` to an accepted-versions list (not replace `2.2`)
- `crossings` fields are only valid in `2.3` specs
- `2.2` specs continue to parse and validate unchanged

### Schema: Contract-Augmented Boundaries

Rather than a separate `crossings` array, contracts attach to existing boundary declarations:

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

  # NEW in 2.3: contract attachments
  contracts:
    - id: "create_user"
      contract: "contracts/create-user.json"    # path relative to module root
      match:                                     # code-site binding
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

### Field Semantics

| Field | Required | Description |
|-------|----------|-------------|
| `id` | yes | Unique within module. Used in diagnostics and cross-module references. |
| `contract` | yes | Path to contract file, relative to module root. Must exist and be non-empty. |
| `match.files` | yes | Glob patterns for files containing this boundary crossing. |
| `match.pattern` | no | Symbol/function name for AST-level binding. If omitted, coverage is file-level only. |
| `direction` | yes | `inbound` (data entering module), `outbound` (data leaving), `bidirectional`. |
| `envelope` | no | `optional` (default) or `required`. When `required`, Specgate checks that code calls an envelope validator. |

### Contract File Format

**Position: format-agnostic for V1, with a parseable-format recommendation.**

Specgate enforces:
- File exists at declared path
- File is non-empty
- File extension is one of: `.json`, `.yaml`, `.yml`, `.ts`, `.zod`, `.proto`

Specgate does NOT parse or validate contract file contents in V1. Future versions may add pluggable schema validators (JSON Schema, Zod AST, protobuf descriptor).

**Rationale:** Vulcan recommended format-agnostic. Athena recommended opinionated (JSON Schema). The compromise: agnostic enforcement with a strong recommendation in docs. This lets teams adopt with existing schemas without migration friction.

### Cross-Module Boundaries

**Position: single owner, optional consumer reference.**

When a boundary spans two modules (producer in A, consumer in B):
- The **owner module** declares the contract with full `match` binding
- Consumer modules **may** reference it: `imports_contract: "api/handlers:create_user"`
- Specgate validates that referenced contracts exist
- Dual declaration is NOT required (Vulcan's position — dual declarations drift)

**Athena's session-types argument** (declare offer/accept on both sides) is theoretically sound but practically too heavy for V1. Revisit in V2 if cross-module contract mismatches become a real failure mode.

## New Violation Types

| Violation ID | Severity | Trigger |
|-------------|----------|---------|
| `boundary.contract_missing` | error | Contract declared but file doesn't exist |
| `boundary.contract_empty` | error | Contract file exists but is empty |
| `boundary.match_unresolved` | error | `match.files` pattern resolves to zero files |
| `boundary.envelope_missing` | warning | `envelope: required` but no envelope validator call found in matched code |
| `boundary.unmatched_crossing` | warning | Advisory: discovered boundary-like pattern in file not covered by any contract (only from `specgate discover`, never from `specgate check`) |

**Implementation note** (from Vulcan): New rule IDs must be added to `KNOWN_CONSTRAINT_RULES` in `src/spec/validation.rs` and wired through `src/cli/mod.rs::boundary_constraint_module`. The `LayerViolation.fix_hint` pattern in `src/rules/layers.rs` should be extended to all violation types.

## Structured Diagnostics

All violations gain optional structured fields for machine consumption:

```json
{
  "ruleId": "boundary.contract_missing",
  "moduleId": "api/handlers",
  "contractId": "create_user",
  "filePath": "src/api/handlers/users.ts",
  "line": 47,
  "expected": "contract file at contracts/create-user.json",
  "actual": "file not found",
  "severity": "error",
  "remediationHint": "Create contracts/create-user.json with the schema for createUser"
}
```

**Output format:** `specgate check --format json` emits NDJSON (one violation per line). Default output remains human-readable. This is additive, not a breaking change.

**Implementation note** (from Vulcan): Current `RuleViolation`/`PolicyViolation`/`VerdictViolation` types have no `expected`/`actual`/`hint` fields. These need to be added as `Option<String>` fields end-to-end through `src/rules/mod.rs` → `src/verdict/mod.rs` → `src/verdict/format.rs`. The existing `fix_hint` on `LayerViolation` (currently dropped in `analyze_project`) should be surfaced as part of this work.

**Verdict schema versioning:** Decouple verdict output schema version from spec-language version. Currently both are hardcoded `"2.2"` in `src/cli/mod.rs` and `src/verdict/mod.rs`. V2 introduces `verdict_schema: "1.0"` as a separate version track.

## Discovery Tool (NOT CI)

`specgate discover` — a local-only command that scans for boundary-like patterns not covered by contracts. This is explicitly NOT part of `specgate check` and NEVER runs in CI.

```bash
# Scan for undeclared boundary crossings
specgate discover

# Output: candidates with confidence scores
# HIGH:   fetch() call in src/api/handlers/billing.ts:23 — no contract covers this file
# MEDIUM: prisma.user.create() in src/db/users.ts:45 — no contract covers this file
# LOW:    process.env.API_KEY in src/config/index.ts:12 — no contract covers this file
```

**Scanner implementation** (informed by Vulcan):
- TS/JS only in V1 (existing AST traversal in `src/parser/mod.rs` supports this)
- High-confidence AST patterns: `fetch()`, `process.env.*`, known SDK imports (axios, prisma) with import-aware matching
- No generic multi-language scanning
- Conservative defaults; false positives are acceptable here because this is a local dev tool, not a gate

### Migration Assistant

```bash
# Generate contract stubs from discovery results
specgate discover --scaffold

# Outputs:
# Created contracts/billing.json (stub — fill in schema)
# Added contract declaration to api/handlers/spec.yaml
# Review and commit when ready
```

Dry-run by default. `--write` to actually create files.

## Envelope Protocol (V2 scope, not deferred)

**Athena's strongest argument:** deferring the envelope is wrong because the envelope IS the proof. Without it, Specgate is reduced to checking that a contract file exists, which is necessary but weak.

**Compromise position:** The envelope is in-scope for this spec but phased after contract declarations. Specgate's role is limited to static verification that envelope validators are called — the runtime validation itself lives in userland.

### What Specgate checks (static)

When `envelope: required` on a contract:
1. The matched file imports an envelope validator function
2. The boundary-crossing code site calls the validator with the correct contract ID
3. Both checks are AST-based, deterministic, and cacheable

### What Specgate does NOT do (runtime)
- Actually validate payloads at runtime
- Generate envelope validators
- Enforce envelope structure

### Envelope library (separate package, not part of Specgate)

A reference implementation that teams can adopt:

```typescript
// specgate-envelope (npm package, separate repo)
import { boundary } from 'specgate-envelope';

// Wraps data in a validated envelope
const result = boundary.validate('create_user', 'v1', rawInput);
// Returns { contractId, schemaVersion, producer, scope, payload } or throws structured error
```

Specgate's static check just looks for the `boundary.validate('create_user', ...)` call pattern in the matched files.

## Implementation Phases

### Phase 1: Spec Language + Parsing (2-3 days)
- Add `contracts` to `Boundaries` struct in `src/spec/types.rs`
- Parse and validate in `src/spec/validation.rs`
- Version compatibility: accept both `2.2` and `2.3`
- Add `match` field validation (files exist, patterns are valid)
- Golden corpus fixtures for contract declarations

### Phase 2: Contract Enforcement Rules (2-3 days)
- `boundary.contract_missing` and `boundary.contract_empty` violations
- `boundary.match_unresolved` violation
- Wire through rules → verdict → CLI pipeline
- Add to `KNOWN_CONSTRAINT_RULES`

### Phase 3: Structured Diagnostics (1-2 days)
- Add `expected`/`actual`/`remediation_hint` to violation types
- `--format json` CLI flag for NDJSON output
- Surface existing `fix_hint` from `LayerViolation`
- Decouple verdict schema version from spec-language version

### Phase 4: Discovery Tool (2-3 days)
- `specgate discover` command (local only)
- AST-based scanner for TS/JS boundary patterns
- Confidence scoring
- `--scaffold` flag for contract stub generation

### Phase 5: Envelope Static Check (2-3 days)
- `envelope: required` field on contracts
- AST check for envelope validator import + call
- `boundary.envelope_missing` violation
- Reference envelope library (separate repo/package)

### Phase 6: CI Gate Integration (1 day)
- `boundary_coverage_minimum` config option
- Coverage = (files with contract-matched crossings) / (files with discovered boundary patterns)
- Only available after Phase 4 (discovery) provides the denominator
- `--since` interaction: coverage gate evaluates blast radius only (consistent with existing behavior)
- Baselineable: `boundary.unmatched_crossing` can be added to baseline for incremental adoption

## What This Proves (Paul's Framework)

After full implementation, Specgate can mechanically prove:

| Claim | How |
|-------|-----|
| Every boundary crossing validates the contract | Contract declarations + match binding + envelope check |
| No drift between schema and enforcement | Single contract file is the source of truth for both declaration and validation |
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
| Version bump strategy | Unresolved | Accept both 2.2 and 2.3; crossings only valid in 2.3 |

## Appendix: Reviewer Disagreements and Resolutions

### Heuristics in CI
- **Athena:** "Heuristics destroy trust in enforcement tools. Ban them from CI entirely."
- **Vulcan:** "Advisory warnings in CI are fine if conservative."
- **Resolution:** Athena wins. CI must be deterministic. Heuristics live in `specgate discover` only.

### Envelope timing
- **Athena:** "Envelope is THE core insight. Phase 1, not deferred."
- **Vulcan:** "Defer envelope. Focus on declarations first."
- **Resolution:** Compromise. Envelope is in-scope but phased after declarations (Phase 5). Athena's argument is theoretically right, but the envelope's static check depends on having contract declarations and match bindings first. You need the skeleton before you can check for the nerves.

### Cross-module contracts
- **Athena:** "Declare in both (session types). Producer offers, consumer accepts."
- **Vulcan:** "Single source of truth. Dual declarations drift."
- **Resolution:** Vulcan wins for V1. Single owner with optional consumer references. Session types are revisitable if cross-module mismatches become a real failure mode.

### Contract file format
- **Athena:** "Be opinionated. Pick JSON Schema."
- **Vulcan:** "Format-agnostic. Enforce existence only."
- **Resolution:** Split the difference. Agnostic enforcement, strong recommendation in docs. Practical teams can adopt with their existing schemas without migration.

### `crossings` as a primitive
- **Athena:** "Wrong abstraction. Conflates transport with data contracts."
- **Vulcan:** "Schema is human-friendly but underspecified."
- **Resolution:** Athena wins on abstraction (removed `kind` enum, transport-agnostic), Vulcan wins on specificity (added `match` field for code-site binding). Renamed from `crossings` to `contracts` to reflect the transport-agnostic nature.
