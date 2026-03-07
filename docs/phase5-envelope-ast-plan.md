# Phase 5: AST Envelope Static Check — Implementation Plan

**Date:** 2026-03-07
**Author:** Lumen (Opus 4.6)
**Repo:** `~/Development/specgate`
**Prerequisite:** Phases 1-4 complete (contracts model, rules, diagnostics, structured output all landed)
**Goal:** When a contract declares `envelope: required`, statically verify that matched source files import and call an envelope validator with the correct contract ID.

---

## What We're Building

The missing piece from Paul Bohm's thesis: proving that data crossing a boundary actually gets validated. Today specgate proves *which modules talk to which* and *that contracts are declared*. Phase 5 proves *that code at the boundary calls a validator*.

### The check (deterministic, AST-based)

For each contract where `envelope: required`:
1. Parse matched files (already in the dependency graph via `oxc_parser`)
2. Verify the file imports the envelope package (configurable, default: `specgate-envelope`)
3. Verify the file contains a call expression matching `boundary.validate('contract_id', ...)` (configurable pattern)
4. Both checks are pure AST traversal — no runtime, no heuristics, fully deterministic

### What this catches

```typescript
// ❌ FAILS: Handler crosses boundary without validation
export async function createUser(req: Request) {
  const body = req.body;  // raw, unvalidated
  await db.users.insert(body);
}

// ✅ PASSES: Handler validates through envelope
import { boundary } from 'specgate-envelope';
export async function createUser(req: Request) {
  const validated = boundary.validate('create_user', req.body);
  await db.users.insert(validated.payload);
}
```

---

## Current State (What Exists)

### Already built
- `EnvelopeRequirement` enum (`Optional` | `Required`) in `src/spec/types.rs` — parsed and serialized
- `boundary.envelope_missing` in `KNOWN_CONSTRAINT_RULES` in `src/spec/validation.rs` — registered
- `collect_call_expression()` in `src/parser/mod.rs` — already walks `CallExpression` nodes via `oxc_parser`
- `MemberExpression` matching (used for `jest.mock` detection) — pattern we can follow
- All contract types, validation, and rule infrastructure from Phases 1-3
- `match.files` glob resolution and `match.pattern` symbol name (declared but pattern match not yet AST-bound)

### Not yet built
- No envelope config section in `SpecConfig`
- No AST analysis for envelope import detection
- No AST analysis for envelope call detection
- No `boundary.envelope_missing` violation producer
- No `specgate discover` command (Phase 4 — we'll build Phase 5 without depending on it)
- No `match.pattern` AST binding (file-level match only currently)

---

## Implementation Tasks

### Task 1: Envelope Config (`src/spec/config.rs`)

Add configurable envelope patterns to `SpecConfig`:

```rust
/// Envelope validation settings for contract enforcement.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct EnvelopeConfig {
    /// Package name to look for in imports (default: "specgate-envelope").
    #[serde(default = "default_envelope_import_pattern")]
    pub import_pattern: String,
    /// Call expression pattern to match (default: "boundary.validate").
    /// Supports dot notation: "boundary.validate" matches `boundary.validate(...)`.
    #[serde(default = "default_envelope_function_pattern")]
    pub function_pattern: String,
}

impl Default for EnvelopeConfig {
    fn default() -> Self {
        Self {
            import_pattern: "specgate-envelope".to_string(),
            function_pattern: "boundary.validate".to_string(),
        }
    }
}
```

Add to `SpecConfig`:
```rust
/// Envelope validation configuration for boundary contracts.
#[serde(default)]
pub envelope: EnvelopeConfig,
```

**Files:** `src/spec/config.rs`
**Tests:** Default values, custom overrides, YAML round-trip

---

### Task 2: Extend Parser — Envelope Call Detection (`src/parser/mod.rs`)

Add new analysis output types:

```rust
/// A detected envelope validator call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvelopeCallInfo {
    /// The contract ID passed as first argument (string literal).
    pub contract_id: String,
    /// Source line number.
    pub line: usize,
    /// Source column.
    pub column: usize,
    /// The full callee pattern matched (e.g., "boundary.validate").
    pub callee: String,
}

/// A detected import of the envelope package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvelopeImportInfo {
    /// The import specifier (package name).
    pub specifier: String,
    /// Source line number.
    pub line: usize,
    /// Local binding name(s) imported.
    pub bindings: Vec<String>,
}
```

Add to `FileAnalysis`:
```rust
pub envelope_imports: Vec<EnvelopeImportInfo>,
pub envelope_calls: Vec<EnvelopeCallInfo>,
```

**Parser changes:**
- In `collect_imports()`: detect imports matching `envelope.import_pattern` config and populate `envelope_imports`
- In `collect_call_expression()`: detect call expressions matching `envelope.function_pattern` config
  - Parse dot-notation pattern: `"boundary.validate"` → match `MemberExpression` where object is `boundary` and property is `validate`
  - Extract first argument if it's a `StringLiteral` → that's the `contract_id`
  - Support both `boundary.validate('id', ...)` and destructured `validate('id', ...)`

**Pattern matching logic:**
```
function_pattern = "boundary.validate"
  → Split on "." → ["boundary", "validate"]
  → Match CallExpression where:
    - callee is MemberExpression
    - object is Identifier("boundary")
    - property is "validate"
  → OR (destructured import):
    - callee is Identifier("validate")
    - AND file has EnvelopeImportInfo with binding "validate" or "boundary"

function_pattern = "validate" (simple)
  → Match CallExpression where:
    - callee is Identifier("validate")
    - AND file imports from envelope package
```

**Files:** `src/parser/mod.rs`
**Tests:** 
- File with `import { boundary } from 'specgate-envelope'; boundary.validate('my_contract', data)` → detected
- File with `import { validate } from 'specgate-envelope'; validate('my_contract', data)` → detected (destructured)
- File with `const { boundary } = require('specgate-envelope'); boundary.validate('my_contract', data)` → detected (CJS)
- File with no envelope import → empty results
- File with envelope import but no matching call → import detected, no call
- File with call but wrong contract ID → call detected with actual ID
- Custom function pattern config
- Renamed import: `import { boundary as b } from 'specgate-envelope'; b.validate(...)` → detected

---

### Task 3: Envelope Rule Engine (`src/rules/contracts.rs`)

Add envelope validation to the existing contract rules:

```rust
/// Check envelope validation for contracts with `envelope: required`.
///
/// For each required-envelope contract:
/// 1. Resolve `match.files` to actual source files
/// 2. Check each matched file's `FileAnalysis` for:
///    a. An `envelope_import` matching the configured package
///    b. An `envelope_call` with `contract_id` matching the contract's `id`
/// 3. Emit `boundary.envelope_missing` if either check fails
pub fn check_envelope_requirements(
    spec: &SpecFile,
    file_analyses: &HashMap<PathBuf, FileAnalysis>,
    config: &EnvelopeConfig,
    project_root: &Path,
) -> Vec<ContractRuleViolation> { ... }
```

**Violation: `boundary.envelope_missing`**
- Severity: warning (as specced in V2 doc)
- Structured fields:
  - `expected`: "Import '{import_pattern}' and call '{function_pattern}' with contract ID '{contract_id}'"
  - `actual`: describes what was found (no import / import but no call / call with wrong ID)
  - `remediation_hint`: specific fix guidance based on what's missing
  - `contract_id`: the contract that requires the envelope

**Blast-radius integration:** Only evaluate envelope checks for contracts in modules affected by `--since` diff. This falls out naturally from the existing contract evaluation pipeline.

**Files:** `src/rules/contracts.rs`
**Tests:**
- Contract with `envelope: required`, matched file has valid envelope call → no violation
- Contract with `envelope: required`, matched file missing import → violation with "missing import" hint
- Contract with `envelope: required`, matched file has import but no call → violation with "missing call" hint
- Contract with `envelope: required`, matched file has call with wrong contract ID → violation with "wrong ID" hint
- Contract with `envelope: optional` → no envelope check at all
- Multiple contracts in same file, one required, one optional → only required checked
- Blast-radius: contract outside diff scope → not evaluated
- Custom envelope config patterns

---

### Task 4: Wire Into Check Pipeline (`src/rules/mod.rs`, `src/cli/check.rs`)

- Thread `EnvelopeConfig` from `SpecConfig` through the rule evaluation pipeline
- Pass `file_analyses` (already computed by parser) to envelope checker
- Collect `boundary.envelope_missing` violations alongside existing contract violations
- Ensure violations flow through verdict builder with proper structured fields
- Verify `--format human` renders envelope violations with helpful output

**Files:** `src/rules/mod.rs`, `src/cli/check.rs` (or `src/cli/mod.rs` check handler)
**Tests:** Integration test: full `specgate check` on a fixture with required envelope → violation in verdict JSON

---

### Task 5: Test Fixtures

Create comprehensive fixtures:

```
tests/fixtures/envelope/
├── valid/
│   ├── modules/
│   │   └── api.spec.yml          # contract with envelope: required
│   ├── contracts/
│   │   └── create-user.json      # contract file
│   ├── src/
│   │   └── api/
│   │       └── handler.ts        # has envelope import + call
│   └── specgate.config.yml
├── missing-import/
│   ├── ...same structure...
│   └── src/api/handler.ts        # no envelope import
├── missing-call/
│   ├── ...same structure...
│   └── src/api/handler.ts        # has import, no call
├── wrong-id/
│   ├── ...same structure...
│   └── src/api/handler.ts        # calls validate('wrong_id', ...)
├── optional-skip/
│   ├── ...same structure...
│   └── modules/api.spec.yml      # envelope: optional
├── custom-pattern/
│   ├── ...same structure...
│   ├── src/api/handler.ts        # uses custom validator
│   └── specgate.config.yml       # custom envelope patterns
└── destructured/
    ├── ...same structure...
    └── src/api/handler.ts        # import { validate } destructured
```

**Test file:** `tests/envelope_checks.rs`

---

### Task 6: Documentation Updates

- `docs/spec-language.md`: Document `envelope` field behavior with Phase 5 semantics
- `docs/getting-started.md`: Add envelope example to the tutorial
- `specgate.config.yml` reference: Document `envelope` config section
- `CHANGELOG.md`: Add Phase 5 entry under `[Unreleased]`
- Update the "What This Proves" table in `docs/specgate-boundary-contracts-v2.md`

---

## Sequencing

```
Task 1 (config) ─────────────────────┐
                                      ├─→ Task 3 (rules) ─→ Task 4 (wiring) ─→ Task 6 (docs)
Task 2 (parser) ─→ Task 5 (fixtures) ┘
```

Tasks 1 and 2 are independent and can be parallel.
Task 3 depends on both 1 and 2.
Task 4 wires everything together.
Task 5 can start with Task 2 (parser fixtures) and expand for Task 3.
Task 6 is last.

---

## Rules

- Branch: `phase5/envelope-ast-check` off `master`
- Atomic commits per task
- `cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test` after each task
- `./scripts/ci/mvp_gate.sh` must pass at the end
- Do NOT modify existing test assertions unless genuinely wrong
- Envelope check is a WARNING not an error (as specced) — it should not break existing CI gates
- The parser changes must not regress performance (envelope detection piggybacks on existing AST traversal, not a second pass)

---

## Estimated Complexity

| Task | Complexity | LOC estimate |
|------|-----------|-------------|
| T1: Config | Low | ~50 |
| T2: Parser | Medium-High | ~200-300 |
| T3: Rules | Medium | ~150-200 |
| T4: Wiring | Low-Medium | ~50-100 |
| T5: Fixtures | Medium | ~200 (YAML/TS fixtures + test file) |
| T6: Docs | Low | ~100 |
| **Total** | | **~750-950** |

This is well within a single Codex session's capacity.

---

## Design Decisions

1. **Warning, not error.** Envelope missing is a warning because teams adopt contracts incrementally. You declare `envelope: required` when you're ready to enforce it. The contract itself (file existence, match resolution) is already an error.

2. **No second AST pass.** Envelope detection hooks into the existing `collect_call_expression` and import collection in `src/parser/mod.rs`. One parse, all data extracted.

3. **Config-driven patterns.** Not everyone uses `specgate-envelope`. The import and function patterns are configurable. Default is the reference implementation's API.

4. **Contract ID as first argument.** This is the binding that makes the check deterministic. We verify that `boundary.validate('create_user', ...)` uses the exact contract ID from the spec. A call to `boundary.validate('wrong_id', ...)` is flagged.

5. **Destructured imports supported.** `import { validate } from 'specgate-envelope'` then `validate('id', data)` works. The parser tracks import bindings and correlates.

6. **CJS require supported.** `const { boundary } = require('specgate-envelope')` is detected via the existing `require_calls` analysis.
