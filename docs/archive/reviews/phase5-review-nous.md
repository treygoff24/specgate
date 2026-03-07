# Phase 5 Review: AST Envelope Static Check

**Reviewer:** Nous (GPT-5.2 Thinking)
**Date:** 2026-03-07
**Plan reviewed:** `docs/phase5-envelope-ast-plan.md`
**Sources cross-referenced:** V2 spec (`docs/specgate-boundary-contracts-v2.md`), `src/parser/mod.rs`, `src/rules/contracts.rs`, `src/spec/types.rs`, `src/spec/config.rs`, `src/rules/mod.rs`, `src/spec/validation.rs`, `src/verdict/mod.rs`, `src/cli/mod.rs`, `src/graph/mod.rs`

---

## 1. Architectural Soundness

### ✅ What fits cleanly

The plan correctly identifies the existing seams and builds on them:

- **`EnvelopeRequirement` enum** already exists in `src/spec/types.rs` (line ~256) with `Optional` | `Required` variants — correctly identified as "already built."
- **`boundary.envelope_missing`** is already registered in `KNOWN_CONSTRAINT_RULES` in `src/spec/validation.rs` (line 30) — the violation ID is pre-wired.
- **`collect_call_expression()`** in `src/parser/mod.rs` (line ~485) already walks `CallExpression` nodes and uses `MemberExpression` matching for `jest.mock` detection — the pattern-matching approach for `boundary.validate` is a natural extension of this existing code.
- **`evaluate_contract_rules()`** in `src/rules/contracts.rs` already has `affected_modules` scoping for blast-radius — envelope checks would inherit this naturally.
- **`ContractRuleViolation`** in `src/rules/contracts.rs` (line ~30) already has `remediation_hint` and `contract_id` fields, and the `analyze_project()` wiring in `src/cli/mod.rs` (lines ~2775-2792) already maps these to `PolicyViolation` — envelope violations can use the same pipeline with zero wiring changes.

### ⚠️ Integration risk: `FileAnalysis` access from the rules layer

**This is the biggest architectural concern in the plan.**

The plan's Task 3 signature is:
```rust
pub fn check_envelope_requirements(
    spec: &SpecFile,
    file_analyses: &HashMap<PathBuf, FileAnalysis>,
    config: &EnvelopeConfig,
    project_root: &Path,
) -> Vec<ContractRuleViolation>
```

But the current `evaluate_contract_rules()` takes `RuleContext<'_>` (line ~67 in `contracts.rs`), which contains `project_root`, `config`, `specs`, and `graph`. `FileAnalysis` is accessible via `graph.files()` → `FileNode.analysis`, but:

1. **`DependencyGraph` does not expose a `HashMap<PathBuf, FileAnalysis>` accessor.** The graph stores `FileNode` objects in a petgraph `DiGraph`. To get file analyses, you'd need to iterate `graph.files()` and build the HashMap yourself, or add an accessor to `DependencyGraph`.

2. **The plan proposes a standalone function signature** that doesn't integrate with `RuleContext`. The existing contract rules use `evaluate_contract_rules(&ctx, affected_modules)` — the envelope check should follow this pattern and be called *inside* `evaluate_contract_rules()` rather than as a separate top-level function.

**Recommendation:** Don't create a separate `check_envelope_requirements()` function. Instead, add envelope checking as an additional loop inside `evaluate_contract_rules()`, right after the existing file/match/ref checks. This avoids:
- A new function signature that doesn't match the existing pattern
- Redundant glob resolution (the match pattern resolution in `check_match_patterns()` already walks the filesystem — envelope checking needs the same resolved file set)
- The need to build a `HashMap<PathBuf, FileAnalysis>` externally

The modified flow would be:
```rust
for contract in &boundaries.contracts {
    // existing checks...
    check_contract_file(ctx, spec, contract);
    check_match_patterns(ctx, spec, contract);
    check_contract_refs(ctx, spec, contract, &contract_registry);
    
    // NEW: envelope check (only if envelope: required)
    if contract.envelope == EnvelopeRequirement::Required {
        check_envelope(ctx, spec, contract, config);
    }
}
```

### ⚠️ Integration risk: `EnvelopeConfig` doesn't exist on `SpecConfig` yet

The plan's Task 1 adds `envelope: EnvelopeConfig` to `SpecConfig`, but `SpecConfig` in `src/spec/config.rs` currently uses `#[serde(deny_unknown_fields)]`... wait, actually checking the code — `SpecConfig` does *not* use `deny_unknown_fields` (only `SpecFile` and `Boundaries` do). So adding `envelope: EnvelopeConfig` with `#[serde(default)]` to `SpecConfig` is safe. Good.

However, **`SpecConfig` is not currently accessible from `RuleContext`** — wait, it *is*: `RuleContext` has a `config: &'a SpecConfig` field (line ~45 in `rules/mod.rs`). So `EnvelopeConfig` would be accessible as `ctx.config.envelope`. This is clean.

### ✅ No second AST pass

The plan correctly notes that envelope detection should piggyback on the existing AST traversal. Since `parse_file()` in `src/parser/mod.rs` does a single pass, adding envelope import/call detection to the same pass is architecturally sound. The `FileAnalysis` struct gets two new fields, and the existing `collect_imports()` (via `extract_module_declarations`) and `collect_call_expression()` get extended — not new functions.

---

## 2. Parser Design

### ✅ Correct overall approach

Extending `collect_call_expression()` and `extract_module_declarations()` is the right call. The existing patterns for `jest.mock` detection (`is_jest_mock_call()` at line ~498) and `require()` detection (`call.common_js_require()` at line ~486) show exactly how to add new call expression matchers.

### ⚠️ Edge case: Computed member expressions

The plan only discusses static member expressions (`boundary.validate`). But what about:

```typescript
const method = 'validate';
boundary[method]('contract_id', data);  // ComputedMemberExpression
```

The plan should explicitly state this is **out of scope** and not matched (which is correct for a deterministic check). The existing `jest.mock` detection also only handles static property names (`member.static_property_name()`), so this is consistent. But the plan should document this non-coverage.

### ⚠️ Edge case: Optional chaining

```typescript
boundary?.validate('contract_id', data);
```

Looking at the parser code, `visit_chain_element()` (line ~409) already handles `ChainElement::CallExpression` and calls `collect_call_expression()`. However, when `boundary?.validate(...)` is an optional chain call, the callee's `MemberExpression` will be wrapped in a `ChainExpression`. The plan doesn't address whether `collect_call_expression()` can extract the member expression from an optional chain callee.

**Recommendation:** Add a test fixture for optional chaining and verify the OXC AST representation. The `call.callee.get_member_expr()` helper used by `is_jest_mock_call()` may or may not work for optional chain callees. If it doesn't, the parser code needs to handle `Expression::ChainExpression` → `ChainElement::StaticMemberExpression` to extract the object/property.

### ⚠️ Edge case: Nested calls / method chaining

```typescript
// Wrapping the result
const result = await boundary.validate('contract_id', data);
// Chained
boundary.validate('contract_id', data).unwrap();
// Nested
processResult(boundary.validate('contract_id', data));
```

The plan handles `await` (the expression visitor already recurses into `AwaitExpression` arguments — line ~369). Chained and nested calls also work because `visit_expression_for_calls()` recursively visits all sub-expressions. **This is fine.**

### ⚠️ Edge case: Aliased re-exports of the envelope module

```typescript
// utils/envelope.ts
export { boundary } from 'specgate-envelope';

// handler.ts  
import { boundary } from '../utils/envelope';
boundary.validate('contract_id', data);
```

In this case, `handler.ts` imports from `../utils/envelope`, not from `specgate-envelope`. The plan's import check (`detect imports matching envelope.import_pattern config`) would **fail** because the import specifier is `../utils/envelope`, not `specgate-envelope`.

This is a real-world pattern. Teams will create wrapper modules around their envelope library.

**Recommendation:** Document this as a known limitation. The Phase 5 check verifies direct imports from the configured package only. Re-exports through local modules are not followed. This could be addressed in a future phase with cross-file import chain resolution (which would require resolver integration, not just parser analysis).

Alternatively, the config could support multiple `import_pattern` values:
```yaml
envelope:
  import_patterns:
    - "specgate-envelope"
    - "../utils/envelope"
```
But this adds complexity. Better to document the limitation and let teams configure their actual import path.

### ⚠️ Destructured import detection needs care

The plan says:
> `import { validate } from 'specgate-envelope'; validate('id', data)` → detected (destructured)

But the plan's logic says:
> Match `Identifier("validate")` AND file has `EnvelopeImportInfo` with binding "validate" or "boundary"

The current `extract_module_declarations()` function (line ~115) processes `ImportDeclaration` nodes but only extracts the `specifier` (package name) and `is_type_only` flag. It does **not** extract individual import bindings (the specific names imported like `{ boundary, validate }`).

The plan acknowledges this by proposing `EnvelopeImportInfo.bindings: Vec<String>`. But implementing binding extraction requires walking `ImportDeclaration.specifiers` to get `ImportSpecifier.local.name` — this is new logic not currently in the parser.

**This is doable** but the plan should note that it's a more invasive parser change than implied. The existing `ImportInfo` struct doesn't track bindings. The plan should clarify: are `bindings` tracked only for envelope imports, or is `ImportInfo` extended for all imports? The former is cleaner for Phase 5 scope.

### ⚠️ CJS `require` detection for envelope

The plan says:
> `const { boundary } = require('specgate-envelope'); boundary.validate('my_contract', data)` → detected (CJS)

Looking at the parser, `call.common_js_require()` (used at line ~486) returns the require specifier. `require_calls` are tracked in `RequireCallInfo` with `specifier`, `line`, `column`. But like imports, the **destructured binding names** are not tracked.

For CJS, detecting `const { boundary } = require('specgate-envelope')` requires inspecting the variable declaration's binding pattern, not just the `require()` call. The plan doesn't detail how CJS binding extraction works. It just says "detected via the existing `require_calls` analysis" — but `require_calls` only has the specifier, not the bindings.

**Recommendation:** For Phase 5, CJS support should be limited to detecting that `require('specgate-envelope')` exists in the file (specifier match), then looking for any matching call expression pattern. Full CJS destructured binding correlation can be a follow-up.

### ⚠️ Renamed imports

The plan lists: `import { boundary as b } from 'specgate-envelope'; b.validate(...)` → detected.

This requires tracking the **local name** vs the **imported name** from the import specifier. OXC's `ImportSpecifier` has both `imported` and `local` fields. The plan should explicitly note that for renamed imports, the matcher needs to use the `local` name (what the code actually uses), not the `imported` name. The existing parser doesn't track either for import bindings.

---

## 3. Rule Engine Completeness

### ⚠️ Conditional envelope calls

```typescript
import { boundary } from 'specgate-envelope';

export async function createUser(req: Request) {
  if (req.body) {
    const validated = boundary.validate('create_user', req.body);
    await db.users.insert(validated.payload);
  } else {
    // no validation on this path!
    await db.users.insert(req.body);
  }
}
```

The plan's AST check is presence-based ("does a call to `boundary.validate('create_user', ...)` exist in this file?"), not control-flow-based. This means a conditional envelope call would **pass** the check even though there's an unvalidated code path.

**This is the correct design choice for Phase 5.** Control-flow analysis is a different class of problem (data-flow/taint analysis) and would massively increase complexity. The plan should explicitly document this limitation: "Phase 5 verifies the *presence* of an envelope call, not that all code paths go through it."

### ⚠️ Envelope calls in helper functions

```typescript
// helpers/validate.ts
import { boundary } from 'specgate-envelope';
export function validateUser(data: unknown) {
  return boundary.validate('create_user', data);
}

// handlers/user.ts (matched file)
import { validateUser } from '../helpers/validate';
export async function createUser(req: Request) {
  const validated = validateUser(req.body);
  // ...
}
```

The matched file (`handlers/user.ts`) doesn't contain a `boundary.validate('create_user', ...)` call — the helper does. The Phase 5 check would flag this as a violation.

**This is a significant real-world pattern.** Teams commonly extract validation into helper functions.

**Options:**
1. **Accept the limitation** and document it. Teams configure `match.files` to point at the helper file instead of the handler. This works but is unintuitive.
2. **Allow configuring the function pattern to match the helper call:** `function_pattern: "validateUser"` — but then you lose the `boundary.validate` specificity.
3. **Cross-file resolution** — way too complex for Phase 5.

**Recommendation:** Accept limitation (1) and document it clearly. The config flexibility (`function_pattern`) partially addresses this: if teams use a consistent wrapper like `validateUser`, they can configure `function_pattern: "validateUser"` and drop the import check requirement (or configure `import_pattern` to match the helper module). The plan should add a section on "Delegated validation" patterns.

### ✅ Multiple envelope calls for the same contract ID

The plan's check is "does at least one matching call exist?" This naturally handles the case where a file calls `boundary.validate('create_user', ...)` multiple times (e.g., in different branches). No issue here.

### ✅ Async patterns

```typescript
const validated = await boundary.validate('create_user', data);
```

The parser already recurses into `AwaitExpression.argument` (line ~369 in `visit_expression_for_calls`). The `await` wrapper is transparent. ✅

---

## 4. Config Design

### ✅ Well-designed defaults

```rust
pub struct EnvelopeConfig {
    pub import_pattern: String,     // default: "specgate-envelope"
    pub function_pattern: String,   // default: "boundary.validate"
}
```

This is appropriately minimal. Two strings, sensible defaults, configurable for teams using custom validators.

### ⚠️ Missing: ability to disable envelope checks entirely

What if a team has `envelope: required` contracts but wants to temporarily disable all envelope checks at the project level? There's no `enabled: bool` on `EnvelopeConfig`.

Currently, you'd have to change every contract from `envelope: required` to `envelope: optional`, which is a lot of spec file changes.

**Recommendation:** Consider adding:
```rust
pub struct EnvelopeConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub import_pattern: String,
    pub function_pattern: String,
}
```

This is low-cost and provides a project-level escape hatch. If deemed unnecessary, at least document that the way to disable is to change contracts to `envelope: optional`.

### ⚠️ Missing: multiple function patterns

Some teams might use different validator functions for different contract types:
```yaml
envelope:
  import_pattern: "specgate-envelope"
  function_patterns:
    - "boundary.validate"
    - "boundary.validateAsync"
```

The plan only supports a single `function_pattern`. This seems fine for Phase 5 — YAGNI. But the field name should perhaps be `function_pattern` (singular) to make it clear, which it already is.

### ⚠️ Should `import_pattern` support scoped packages?

The default is `"specgate-envelope"`. But scoped packages like `"@company/specgate-envelope"` should also work. Since the match is a simple string comparison against the import specifier, scoped packages work naturally. ✅ No issue.

---

## 5. Test Coverage Gaps

### Missing from the fixture plan:

1. **Optional chaining:** `boundary?.validate('contract_id', data)` — tests whether the parser handles chain expressions.

2. **Template literal contract ID:** `boundary.validate(\`create_user\`, data)` — the plan only checks for `StringLiteral` first arguments. Template literals without expressions are semantically equivalent but are a different AST node (`TemplateLiteral` with zero expressions). Should this be handled? Probably yes — it's common in TypeScript.

3. **Variable contract ID:** `const id = 'create_user'; boundary.validate(id, data)` — this should explicitly NOT match and be documented.

4. **No-argument call:** `boundary.validate()` — the parser should handle this gracefully (no contract ID extracted, no match).

5. **TypeScript `as const` patterns:** `boundary.validate('create_user' as const, data)` — the `as const` wraps the string in a `TSAsExpression`. The plan's parser code needs to unwrap TS type assertions to find the underlying string literal.

6. **Multi-contract file:** A file that has envelope calls for TWO different contract IDs. Both contracts reference the same file in `match.files`. The check should pass for both independently.

7. **Envelope call with wrong function but right package:** `import { boundary } from 'specgate-envelope'; boundary.schema('create_user', data)` — should NOT match because `function_pattern` is `boundary.validate`, not `boundary.schema`.

8. **Type-only import:** `import type { boundary } from 'specgate-envelope'` — should NOT count as a valid envelope import (type-only imports are erased at runtime).

9. **Namespace import:** `import * as envelope from 'specgate-envelope'; envelope.boundary.validate('id', data)` — the plan doesn't cover namespace imports at all.

10. **Dynamic import of envelope package:** `const { boundary } = await import('specgate-envelope')` — should this count? Probably not for Phase 5, but document the limitation.

11. **Side-effect import:** `import 'specgate-envelope'` — no bindings, should not count.

### ⚠️ Integration test gap

The plan mentions "Integration test: full `specgate check` on a fixture with required envelope → violation in verdict JSON" but doesn't specify testing the **human-readable output** format for envelope violations. The human format renderer needs to handle the new violation type.

---

## 6. Sequencing & Risk

### ✅ Dependency DAG is correct

```
Task 1 (config) ─────────────────────┐
                                      ├─→ Task 3 (rules) ─→ Task 4 (wiring) ─→ Task 6 (docs)
Task 2 (parser) ─→ Task 5 (fixtures) ┘
```

Tasks 1 and 2 are independent. Task 3 needs both. This is correct.

### ⚠️ Hidden dependency: `DependencyGraph` accessor

Task 3 (rules) needs to access `FileAnalysis` from the `DependencyGraph`. As noted in Section 1, there's no direct `HashMap<PathBuf, FileAnalysis>` accessor on `DependencyGraph`. Either:
- Add a `file_analyses()` method to `DependencyGraph` (in `src/graph/mod.rs`)
- Build the map inside `evaluate_contract_rules()` by iterating `graph.files()`

This is a Task 3 dependency on `src/graph/mod.rs` that isn't called out in the plan. It's low-risk but should be acknowledged.

### ⚠️ Risk: Performance regression from glob re-evaluation

The current `check_match_patterns()` in `contracts.rs` (line ~140) uses `find_matching_files()` which walks the entire project directory with `walkdir`. The envelope check also needs to resolve `match.files` patterns to actual files. If implemented as a separate function, this means **two filesystem walks per contract** — once for the match pattern check and once for the envelope check.

**Recommendation:** Refactor `check_match_patterns()` to return the resolved file paths (not just pass/fail), then pass them to the envelope checker. This avoids the double walk.

### ✅ LOC estimate is reasonable

750-950 LOC for the full phase is realistic. The parser changes (200-300 LOC) are the heaviest lift, which is appropriate — that's where the new AST analysis logic lives.

---

## 7. Spec Consistency

### ✅ Matches V2 spec on core semantics

- V2 spec says `boundary.envelope_missing` is severity **warning** (violation table) — plan correctly says "Warning, not error" (Design Decisions §1).
- V2 spec says envelope check requires import + call with contract ID — plan matches.
- V2 spec says configurable via `specgate.config.yml` `envelope` section — plan matches.
- V2 spec says Phase 5 timing (after contract declarations) — plan matches.

### ⚠️ Inconsistency: Severity in wiring

The plan says envelope violations are **warnings**, and the V2 spec agrees. But looking at the current `analyze_project()` wiring in `src/cli/mod.rs` (line ~2780), ALL contract violations are mapped with `severity: Severity::Error`:

```rust
let contract_violations = crate::rules::evaluate_contract_rules(&ctx, affected_modules)
    .into_iter()
    .map(|contract_violation| PolicyViolation {
        rule: contract_violation.violation.rule,
        severity: Severity::Error,  // ← hardcoded Error!
        ...
    })
```

If `boundary.envelope_missing` flows through this same pipeline, it will be emitted as an **error**, not a **warning**. This contradicts both the plan and the V2 spec.

**Recommendation:** The `ContractRuleViolation` struct (or the inner `RuleViolation`) needs a `severity` field, or the `analyze_project()` mapping needs to check the rule ID and assign appropriate severity:

```rust
let severity = match contract_violation.violation.rule.as_str() {
    "boundary.envelope_missing" => Severity::Warning,
    _ => Severity::Error,
};
```

This is a bug-in-waiting that the plan doesn't address.

### ⚠️ Missing: Version gating for envelope enforcement

The V2 spec says contracts are only valid in `2.3` specs. Should `envelope: required` be similarly gated? Can a `2.2` spec with empty contracts somehow trigger envelope checks? No — contracts only exist in `2.3` specs, so the gating is implicit. But the plan should note this.

### ⚠️ V2 spec mentions `match.pattern` for "symbol/function name (AST-matched)"

The V2 spec says `match.pattern` is for "Symbol/function name for AST-level binding." Phase 5 introduces a *different* kind of AST matching (envelope call detection). The plan should clarify the relationship between `match.pattern` (which narrows the code site to a specific function) and the envelope check (which looks for validator calls). Are they complementary? If `match.pattern: "createUser"` is set, does the envelope check only look for `boundary.validate('create_user', ...)` within the `createUser` function? Or is it file-level regardless?

The plan says file-level. This is fine but should be explicitly stated since `match.pattern` creates an expectation of function-level scoping.

---

## 8. Summary of Recommendations

### Must-fix before implementation:

1. **Severity wiring bug:** `analyze_project()` hardcodes `Severity::Error` for all contract violations. Envelope violations must be `Warning`. Add severity to `ContractRuleViolation` or add rule-based severity mapping.

2. **Don't create a standalone `check_envelope_requirements()` function.** Integrate into `evaluate_contract_rules()` to reuse the existing file resolution from `check_match_patterns()` and avoid a redundant filesystem walk.

3. **Add `DependencyGraph` accessor** for getting `FileAnalysis` by path, or document how the graph is accessed from the contract rules layer.

### Should-fix (high value, low cost):

4. **Type-only import exclusion.** `import type { boundary } from 'specgate-envelope'` must not count as a valid envelope import. The parser already tracks `is_type_only` on `ImportInfo`.

5. **Template literal contract IDs.** Handle `TemplateLiteral` with zero expressions (static template) as equivalent to `StringLiteral`.

6. **TSAsExpression unwrapping.** `'create_user' as const` should be unwrapped to find the underlying string literal.

7. **Optional chaining test.** Add a fixture and verify `boundary?.validate(...)` works through the chain expression handler.

### Nice-to-have (document or defer):

8. **Document**: computed member expressions, aliased re-exports, helper function delegation, variable contract IDs, namespace imports, and dynamic imports of envelope package are all explicitly out of scope.

9. **Consider** `enabled: bool` on `EnvelopeConfig` for project-level disable.

10. **Consider** refactoring `check_match_patterns()` to return resolved paths for reuse by envelope checking.

---

## 9. Verdict

**The plan is architecturally sound and well-structured.** It correctly builds on existing infrastructure, follows established patterns in the codebase, and makes good design trade-offs (presence-based not control-flow-based, configurable patterns, warning not error).

**The severity wiring issue (#1 above) is a real bug** that would cause envelope violations to fail CI instead of being advisory warnings — directly contradicting the plan's own Design Decision §1 and the V2 spec. This must be fixed.

The parser edge cases (#5, #6, #7) are low-risk but would cause false negatives for common TypeScript patterns. They're worth addressing in Task 2.

Overall: **ready to implement with the corrections above.**
