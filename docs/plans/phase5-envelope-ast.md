# Phase 5: AST Envelope Static Check — Implementation Plan v2

> **Historical note:** This is the implementation plan that led to the shipped
> envelope AST check. Use `docs/reference/envelope-guide.md`,
> `docs/reference/spec-language.md`, and `docs/roadmap.md` for current
> operator-facing truth.

**Date:** 2026-03-07
**Author:** Lumen (Opus 4.6), synthesized from reviews by Athena (Gemini 3.1 Pro) and Nous (GPT 5.4)
**Repo:** `~/Development/specgate`
**Branch:** `phase5/envelope-ast-check`
**Prerequisite:** Phases 1-4 complete (contracts model, rules, diagnostics, structured output all landed; 420 tests green)
**Goal:** When a contract declares `envelope: required`, statically verify that matched source files import and call an envelope validator with the correct contract ID.

---

## What We're Building

The missing piece from Paul Bohm's thesis: proving that data crossing a boundary actually gets validated.

Today specgate proves *which modules talk to which* and *that contracts are declared*. Phase 5 proves *that code at the boundary calls a validator*.

```typescript
// ❌ FAILS: Handler crosses boundary without validation
export async function createUser(req: Request) {
  const body = req.body;
  await db.users.insert(body);
}

// ✅ PASSES: Handler validates through envelope
import { boundary } from 'specgate-envelope';
export async function createUser(req: Request) {
  const validated = boundary.validate('create_user', req.body);
  await db.users.insert(validated.payload);
}
```

After Phase 5, specgate mechanically proves the full chain:
1. Every module boundary is declared (spec files)
2. Every import respects those boundaries (import-graph analysis)
3. Every boundary crossing has a contract (contract declarations + match binding)
4. **Every contract is validated at runtime** (envelope static check) ← NEW

---

## Architecture Decision: Targeted Second Pass, Not Parser Extension

**Athena's review identified a critical flaw in v1:** the original plan proposed injecting envelope detection into `src/parser/mod.rs`. This is wrong for two reasons:

1. **Config plumbing:** `parse_file()` has no access to `SpecConfig`. Threading project config through the generic dependency graph builder violates separation of concerns.
2. **Wasted CPU:** `parse_file()` runs on every file in the project. Envelope checking only applies to the handful of boundary files matched by contracts with `envelope: required`.

**v2 approach:** Keep `src/parser/mod.rs` strictly about generic dependency edges. Perform a **targeted second AST pass** inside the rules layer, only on files resolved by `match.files` for contracts with `envelope: required`. OXC parses a typical TS file in <0.1ms; re-parsing 5 boundary files costs nothing.

This creates a new module `src/rules/envelope.rs` that:
- Takes a file path + the envelope config
- Parses the file with `oxc_parser`
- Walks the AST looking for envelope imports and calls
- Returns structured results

The rules layer calls this only when needed. The generic parser stays clean.

---

## Architecture Decision: `match.pattern` Scoping (Close the Loophole)

**Athena's review identified a dangerous false-negative loophole in v1:** file-level checking means if `users.ts` exports both `createUser` and `deleteUser`, and only `createUser` calls the validator, the whole file passes. `deleteUser` crosses the boundary unvalidated.

**v2 approach:** When `match.pattern` is present on a contract, the envelope check is **scoped to the AST subtree of that specific exported function**. The check verifies that `boundary.validate('contract_id', ...)` occurs inside the function body of `createUser`, not just anywhere in the file.

When `match.pattern` is omitted, file-level is the fallback (porous but acceptable for less granular contracts).

**Implementation:** The envelope analyzer walks the AST to find the exported function/const matching `match.pattern`, then restricts the call expression search to that function's body span.

---

## Architecture Decision: Severity Wiring Fix

**Nous identified a real bug:** `analyze_project()` in `src/cli/mod.rs` (line ~2779) hardcodes `Severity::Error` for ALL contract violations. If `boundary.envelope_missing` flows through this pipeline, it breaks CI instead of being an advisory warning.

**v2 approach:** Add a `severity` field to `ContractRuleViolation`. The envelope checker sets `Severity::Warning`. The existing contract checks continue to set `Severity::Error`. The `analyze_project()` mapping uses the violation's severity instead of hardcoding.

---

## Design Decisions (Documented Tradeoffs)

### Why require a wrapper package?

Teams will ask why they can't just use `zod.parse()` or `joi.validate()` directly. The answer: without a contract ID as a string literal first argument, specgate cannot deterministically link the validation call in the code to the specific contract declared in the YAML. The string literal `'create_user'` is the anchor that makes static AST analysis possible. If we looked for `.parse()`, we'd need complex type-tracing to prove `userSchema` corresponds to the `create_user` contract. The wrapper exists to make the link mechanical.

The wrapper is configurable (`function_pattern` / `import_pattern`), so teams using custom validators configure their own patterns. The `specgate-envelope` default is a reference implementation, not a hard requirement.

### Presence-based, not control-flow

Phase 5 verifies the *presence* of an envelope call, not that all code paths go through it. A conditional call inside an `if` block passes the check even if there's an unvalidated else branch. Control-flow analysis (taint tracking) is a fundamentally different class of problem and out of scope. This is a deliberate design boundary.

### No cross-file resolution

If validation happens in a helper function (`validateUser()` in `helpers/validate.ts`) and the matched handler file calls the helper, the check fails because the matched file doesn't contain `boundary.validate()` directly. Teams have two options:
- Point `match.files` at the helper file where validation actually happens
- Configure `function_pattern` to match the helper function name (e.g., `"validateUser"`) and `import_pattern` to match the helper module path

Cross-file import chain resolution is out of scope for Phase 5.

### Explicit non-coverage (out of scope)

These patterns are intentionally not detected and should be documented:
- Computed member expressions: `boundary['validate'](...)`
- Aliased re-exports through local modules: `import { boundary } from '../utils/envelope'` (configure `import_pattern` as workaround)
- Decorator patterns: `@Validate` on class methods
- Variable contract IDs: `const id = 'create_user'; boundary.validate(id, data)`
- Namespace imports: `import * as envelope from 'specgate-envelope'`
- Dynamic imports: `const { boundary } = await import('specgate-envelope')`
- Middleware validation in a separate file (use `match.files` workaround)

---

## Implementation Tasks

### Task 1: Envelope Config (`src/spec/config.rs`)

Add configurable envelope patterns to `SpecConfig`:

```rust
/// Envelope validation settings for contract enforcement.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct EnvelopeConfig {
    /// Master switch to enable/disable envelope checking project-wide.
    /// When false, all `envelope: required` contracts are treated as optional.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Package name(s) to look for in imports.
    /// Supports multiple patterns for teams with wrapper modules.
    /// Default: ["specgate-envelope"]
    #[serde(default = "default_envelope_import_patterns")]
    pub import_patterns: Vec<String>,
    /// Call expression pattern to match.
    /// Supports dot notation: "boundary.validate" matches `boundary.validate(...)`.
    /// Default: "boundary.validate"
    #[serde(default = "default_envelope_function_pattern")]
    pub function_pattern: String,
}

impl Default for EnvelopeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            import_patterns: vec!["specgate-envelope".to_string()],
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

**Config YAML example:**
```yaml
# specgate.config.yml
envelope:
  enabled: true
  import_patterns:
    - "specgate-envelope"
    - "@myorg/validation"        # team's custom wrapper
    - "./src/utils/envelope"     # local re-export module
  function_pattern: "boundary.validate"
```

**Files:** `src/spec/config.rs`
**Tests:** Default values, custom overrides, YAML round-trip, `enabled: false` disables all checks

---

### Task 2: Envelope AST Analyzer (`src/rules/envelope.rs` — NEW MODULE)

Create a new module that performs targeted AST analysis on specific files. This does NOT modify `src/parser/mod.rs`.

```rust
/// Result of analyzing a single file for envelope compliance.
#[derive(Debug, Clone)]
pub struct EnvelopeAnalysis {
    /// Whether the file imports the envelope package (any of the configured patterns).
    pub has_envelope_import: bool,
    /// Local binding names imported from the envelope package.
    pub import_bindings: Vec<String>,
    /// Whether the import is type-only (should NOT count).
    pub is_type_only_import: bool,
    /// Envelope calls found, each with contract ID and source location.
    pub calls: Vec<EnvelopeCall>,
}

#[derive(Debug, Clone)]
pub struct EnvelopeCall {
    pub contract_id: String,
    pub line: usize,
    pub column: usize,
    /// Byte offset span of the call expression (for match.pattern scoping).
    pub span: (u32, u32),
}
```

**Core function:**
```rust
/// Parse a file and extract envelope information.
/// This is a targeted second AST pass — only called on files matched
/// by contracts with `envelope: required`.
pub fn analyze_file_for_envelope(
    path: &Path,
    config: &EnvelopeConfig,
) -> Result<EnvelopeAnalysis, EnvelopeError> { ... }
```

**AST walking logic:**

1. **Import detection:**
   - Walk `ImportDeclaration` nodes
   - Check if `source.value` matches any of `config.import_patterns`
   - Skip if `import_kind.is_type()` (type-only imports don't count — erased at runtime)
   - Extract `local` binding names from `ImportSpecifier` nodes (handles destructured and renamed imports)
   - Also check `require()` calls: if specifier matches config patterns, mark as imported

2. **Call expression detection:**
   - Parse `config.function_pattern` on dots: e.g., `"boundary.validate"` → `["boundary", "validate"]`
   - For dot-notation patterns, match `CallExpression` where callee is a `StaticMemberExpression` chain
   - For single-name patterns (e.g., `"validate"`), match `CallExpression` where callee is `Identifier("validate")` AND the identifier was imported from the envelope package
   - Extract first argument as contract ID:
     - `StringLiteral` → use `.value` directly
     - `TemplateLiteral` with zero expressions → use the static string (Nous finding #5)
     - `TSAsExpression` wrapping a `StringLiteral` → unwrap to find the string (Nous finding #6, handles `'id' as const`)
     - Anything else (variable, binary expression, call) → not matched (intentional)
   - Handle optional chaining: when callee is inside a `ChainExpression`, unwrap to find the member expression (Nous finding #7)
   - Record `(contract_id, line, column, span)` for each matched call

3. **Renamed import correlation:**
   - Track `import { boundary as b }` → local name is `b`
   - When matching `b.validate(...)`, check if `b` is in `import_bindings` for an envelope package import

**Files:** `src/rules/envelope.rs` (new), `src/rules/mod.rs` (add `pub mod envelope;`)
**Tests (unit, in the module):**
- Standard ESM import + call → detected
- Destructured import: `import { validate } from 'specgate-envelope'; validate('id', data)` → detected
- Renamed import: `import { boundary as b } from 'specgate-envelope'; b.validate('id', data)` → detected
- CJS require: `const { boundary } = require('specgate-envelope'); boundary.validate('id', data)` → detected (specifier match + call match)
- Type-only import: `import type { boundary } from 'specgate-envelope'` → NOT counted
- Side-effect import: `import 'specgate-envelope'` → NOT counted (no bindings)
- Template literal ID: `` boundary.validate(`create_user`, data) `` → detected
- `as const` assertion: `boundary.validate('create_user' as const, data)` → detected (unwrap TSAsExpression)
- Optional chaining: `boundary?.validate('create_user', data)` → detected
- No-argument call: `boundary.validate()` → no contract ID, not matched
- Wrong function: `boundary.schema('id', data)` → NOT matched
- Variable contract ID: `boundary.validate(id, data)` → NOT matched
- No envelope import at all → `has_envelope_import: false`
- Custom config patterns

---

### Task 3: `match.pattern` Function Scoping (`src/rules/envelope.rs`)

When a contract has `match.pattern` (e.g., `pattern: "createUser"`), the envelope check must verify the call occurs INSIDE that function, not just anywhere in the file.

```rust
/// Find the byte span of the exported function matching the pattern name.
/// Returns None if no matching export is found.
pub fn find_function_span(
    source: &str,
    program: &oxc_ast::ast::Program,
    pattern: &str,
) -> Option<(u32, u32)> { ... }
```

**Logic:**
- Walk top-level statements and exported declarations
- Match `export function createUser(...)` → return function body span
- Match `export const createUser = (...)` → return arrow/function body span
- Match `export default function createUser(...)` → return body span
- Match `export { createUser }` with a preceding `function createUser(...)` declaration → return body span

**Integration with envelope check:**
```rust
if let Some(pattern) = &contract.r#match.pattern {
    // Scope: only calls INSIDE this function count
    let fn_span = find_function_span(source, &program, pattern);
    calls.retain(|call| {
        if let Some((start, end)) = fn_span {
            call.span.0 >= start && call.span.1 <= end
        } else {
            false // function not found → no calls match
        }
    });
}
// else: file-level — all calls in the file count
```

**Files:** `src/rules/envelope.rs`
**Tests:**
- File with two exports, only one has envelope call, `match.pattern` selects the right one → pass
- File with two exports, `match.pattern` selects the one WITHOUT envelope call → fail
- No `match.pattern` → file-level, any call in file counts
- `match.pattern` function not found in file → violation (function not exported)
- Arrow function export: `export const handler = async () => { boundary.validate(...) }`

---

### Task 4: Integrate Into Contract Rules (`src/rules/contracts.rs`)

Wire the envelope analyzer into the existing contract evaluation pipeline.

**Key changes:**

1. **Add severity to `ContractRuleViolation`:**
```rust
pub struct ContractRuleViolation {
    pub violation: RuleViolation,
    pub remediation_hint: String,
    pub contract_id: String,
    pub severity: Severity,  // NEW — was hardcoded in analyze_project()
}
```

2. **Refactor `check_match_patterns()` to return resolved file paths:**
Currently returns `Option<ContractRuleViolation>` (pass/fail). Refactor to return `(Option<ContractRuleViolation>, Vec<PathBuf>)` so the resolved paths can be reused by the envelope checker without a second filesystem walk.

3. **Add envelope check inside the contract loop:**
```rust
for contract in &boundaries.contracts {
    // Existing checks...
    if let Some(v) = check_contract_file(ctx, spec, contract) {
        violations.push(v);
    }

    let (match_violation, resolved_files) = check_match_patterns(ctx, spec, contract);
    if let Some(v) = match_violation {
        violations.push(v);
    }

    if let Some(v) = check_contract_refs(ctx, spec, contract, &contract_registry) {
        violations.push(v);
    }

    // NEW: Envelope check (only if required AND envelope checking is enabled)
    if contract.envelope == EnvelopeRequirement::Required
        && ctx.config.envelope.enabled
        && !resolved_files.is_empty()
    {
        let envelope_violations = check_envelope(
            ctx, spec, contract, &resolved_files, &ctx.config.envelope,
        );
        violations.extend(envelope_violations);
    }
}
```

4. **`check_envelope()` implementation:**
```rust
fn check_envelope(
    ctx: &RuleContext<'_>,
    spec: &SpecFile,
    contract: &BoundaryContract,
    resolved_files: &[PathBuf],
    config: &EnvelopeConfig,
) -> Vec<ContractRuleViolation> {
    let mut violations = Vec::new();
    
    for file_path in resolved_files {
        let analysis = match envelope::analyze_file_for_envelope(file_path, config) {
            Ok(a) => a,
            Err(_) => continue, // file read/parse error — skip gracefully
        };
        
        // Check 1: Must have non-type-only envelope import
        if !analysis.has_envelope_import || analysis.is_type_only_import {
            violations.push(ContractRuleViolation {
                violation: RuleViolation { ... },
                remediation_hint: format!(
                    "Add `import {{ boundary }} from '{}'` to {}",
                    config.import_patterns.first().unwrap_or(&"specgate-envelope".to_string()),
                    file_path.display()
                ),
                contract_id: contract.id.clone(),
                severity: Severity::Warning,  // WARNING, not error
            });
            continue;
        }
        
        // Check 2: Must have a call with the correct contract ID
        let mut matching_calls = analysis.calls.iter()
            .filter(|c| c.contract_id == contract.id)
            .collect::<Vec<_>>();
        
        // Check 3: If match.pattern is present, scope to that function
        if let Some(pattern) = &contract.r#match.pattern {
            // Re-parse to get function span (or cache from analyze_file_for_envelope)
            // Filter calls to only those inside the function body
            matching_calls.retain(|call| /* inside function span */);
        }
        
        if matching_calls.is_empty() {
            let actual = if analysis.calls.is_empty() {
                "no envelope validation calls found".to_string()
            } else {
                let found_ids: Vec<_> = analysis.calls.iter()
                    .map(|c| c.contract_id.as_str()).collect();
                format!("found calls for contract IDs {:?}, but not '{}'", found_ids, contract.id)
            };
            
            violations.push(ContractRuleViolation {
                violation: RuleViolation { ... },
                remediation_hint: format!(
                    "Add `boundary.validate('{}', data)` call in {}",
                    contract.id, file_path.display()
                ),
                contract_id: contract.id.clone(),
                severity: Severity::Warning,
            });
        }
    }
    
    violations
}
```

5. **Fix severity wiring in `analyze_project()`** (`src/cli/mod.rs` line ~2779):
```rust
// BEFORE (hardcoded):
severity: Severity::Error,

// AFTER (from violation):
severity: contract_violation.severity,
```

**Files:** `src/rules/contracts.rs`, `src/rules/mod.rs`, `src/cli/mod.rs`
**Tests:**
- Full pipeline: contract with `envelope: required`, file has valid call → no violation
- Full pipeline: missing import → warning violation with hint
- Full pipeline: import present but no call → warning violation
- Full pipeline: call with wrong contract ID → warning violation listing what was found
- Full pipeline: `envelope: optional` → no check
- Full pipeline: `envelope.enabled: false` in config → no check even if required
- Multi-contract file: two contracts reference same file, both required → each checked independently
- Blast-radius: contract outside `--since` diff → not evaluated
- Severity: envelope violations are Warning, other contract violations remain Error
- `match.pattern` scoping: call inside matched function → pass; call outside → fail

---

### Task 5: Test Fixtures & Integration Tests

**Fixture structure:**
```
tests/fixtures/envelope/
├── valid-basic/                    # standard import + call
├── valid-destructured/             # import { validate }, call validate('id', data)
├── valid-renamed/                  # import { boundary as b }, call b.validate(...)
├── valid-cjs/                      # require('specgate-envelope'), boundary.validate(...)
├── valid-template-literal/         # boundary.validate(`create_user`, data)
├── valid-as-const/                 # boundary.validate('create_user' as const, data)
├── valid-optional-chaining/        # boundary?.validate('create_user', data)
├── valid-match-pattern/            # match.pattern scoped to correct function
├── missing-import/                 # no envelope import at all
├── missing-call/                   # import present, no validate call
├── wrong-id/                       # call with wrong contract ID
├── type-only-import/               # import type { boundary } — should fail
├── optional-skip/                  # envelope: optional — no check
├── disabled-config/                # envelope.enabled: false — no check
├── multi-contract/                 # two contracts in one file, both required
├── match-pattern-wrong-fn/         # match.pattern selects function WITHOUT call
├── custom-pattern/                 # custom import_patterns + function_pattern
└── human-format/                   # for testing human-readable output rendering
```

Each fixture has:
- `specgate.config.yml`
- `modules/*.spec.yml`
- `contracts/*.json`
- `src/**/*.ts`

**Test file:** `tests/envelope_checks.rs`
**Integration test:** full `specgate check` → verify verdict JSON has correct violations, severity, structured fields
**Human output test:** verify `--format human` renders envelope violations correctly

---

### Task 6: Documentation Updates

- **`docs/reference/spec-language.md`:** Document `envelope` field behavior, `match.pattern` scoping semantics, and the warning severity
- **`docs/reference/getting-started.md`:** Add envelope example to the tutorial
- **`specgate.config.yml` reference:** Document `envelope` config section with all fields
- **`CHANGELOG.md`:** Add Phase 5 entry under `[Unreleased]`
- **`docs/design/boundary-contracts-v2.md`:** Update "What This Proves" table
- **NEW `docs/reference/envelope-guide.md`:** Dedicated guide explaining:
  - Why the wrapper is required (string literal anchors static analysis)
  - How to configure for custom validators
  - Known limitations (presence-based, no cross-file, no control-flow)
  - Workarounds for common patterns (middleware, helpers, decorators)
  - The `enabled: false` escape hatch

---

## Sequencing

```
Task 1 (config) ────────────────────────────┐
                                             ├─→ Task 4 (integration) ─→ Task 6 (docs)
Task 2 (analyzer) ─→ Task 3 (fn scoping) ──┘
                   └─→ Task 5 (fixtures — start early, expand with T3/T4)
```

- Tasks 1 and 2 are independent, can be parallel
- Task 3 (function scoping) depends on Task 2 (it extends the analyzer)
- Task 4 (integration) depends on Tasks 1, 2, and 3
- Task 5 fixtures can start with Task 2 unit tests and expand as Tasks 3-4 land
- Task 6 is last

---

## Rules

- Branch: `phase5/envelope-ast-check` off `master`
- Atomic commits per task
- `cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test` after each task
- `./scripts/ci/mvp_gate.sh` must pass at the end
- Do NOT modify existing test assertions unless genuinely wrong
- Envelope violations are **WARNING**, not error — they must NOT break existing CI gates
- Do NOT modify `src/parser/mod.rs` — envelope analysis lives in `src/rules/envelope.rs`

---

## Estimated Complexity

| Task | Complexity | LOC estimate |
|------|-----------|-------------|
| T1: Config | Low | ~60 |
| T2: Envelope analyzer | High | ~350-450 |
| T3: Function scoping | Medium | ~100-150 |
| T4: Integration + severity fix | Medium | ~150-200 |
| T5: Fixtures + tests | Medium-High | ~300-400 |
| T6: Docs | Low-Medium | ~150-200 |
| **Total** | | **~1100-1450** |

Larger than v1 due to the function scoping addition and the targeted analyzer being a standalone module. Still within a single focused Codex session.

---

## Review Incorporation Summary

| Finding | Source | Resolution |
|---------|--------|-----------|
| Don't put envelope detection in `src/parser/mod.rs` | Athena | ✅ New `src/rules/envelope.rs` module with targeted second pass |
| `match.pattern` function scoping required | Athena | ✅ Task 3 — scope envelope check to matched function's AST subtree |
| Severity wiring bug (hardcoded Error) | Nous | ✅ Add severity field to `ContractRuleViolation`, fix `analyze_project()` |
| Integrate into `evaluate_contract_rules()` | Nous | ✅ Task 4 — envelope check inside existing contract loop |
| Reuse resolved file paths from `check_match_patterns()` | Nous | ✅ Refactor to return `(violation, resolved_paths)` |
| Type-only imports must not count | Nous | ✅ Task 2 — check `import_kind.is_type()` |
| Template literal contract IDs | Nous | ✅ Task 2 — handle `TemplateLiteral` with zero expressions |
| `as const` unwrapping | Nous | ✅ Task 2 — unwrap `TSAsExpression` |
| Optional chaining support | Nous | ✅ Task 2 — unwrap `ChainExpression` |
| `enabled: bool` on EnvelopeConfig | Nous | ✅ Task 1 — project-level escape hatch |
| Multiple `import_patterns` | Nous/Athena | ✅ Task 1 — `import_patterns: Vec<String>` |
| Document wrapper requirement aggressively | Athena | ✅ Task 6 — dedicated `envelope-guide.md` |
| Document all non-coverage explicitly | Both | ✅ Design Decisions section + Task 6 |
| Multi-contract file test | Athena | ✅ Task 5 fixture |
| Human format rendering test | Nous | ✅ Task 5 fixture |
| `DependencyGraph` accessor question | Nous | ✅ Resolved — `graph.file(path)` already provides `FileNode.analysis` |
| Version gating (implicit via 2.3) | Nous | ✅ Documented — contracts only exist in 2.3 specs |
