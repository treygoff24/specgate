# Phase 5 Build Review: Envelope AST Static Check (Athena)

## 1. The "targeted second AST pass" Architecture
**Verdict: Spot on.**
The decision to decouple the envelope AST parsing from the core module dependency graph parser was the right call. `src/rules/envelope.rs` spins up an `oxc_parser` pass on demand strictly for files matched by `contract.match.files` when `envelope: required` is set. This keeps the fast-path dependency graph unaffected by the heavier AST traversal needed for granular statement inspection. The analyzer is self-contained and avoids bloating the global rule context.

## 2. `match.pattern` Scoping & Limitations
**Verdict: Clean start, but misses critical real-world TS patterns.**
`find_exported_function_span()` does a solid job of mapping standard direct exports (`export function`, `export const fn = () => {}`, `export default function`).

However, it has two major blind spots that will cause immediate **false positives** in production codebases:
1. **Higher-Order Functions / Wrappers:**
   ```typescript
   export const createUser = withAuth(async (req) => {
     boundary.validate('create_user', req.body);
   });
   ```
   The AST matcher specifically looks for an `ArrowFunctionExpression` or `FunctionExpression` in the variable declarator init. It will completely miss a `CallExpression` wrapping the handler, returning `None` for the span, and thus throwing a `boundary.envelope_missing` warning even though the contract is satisfied.

2. **Indirect Exports:**
   ```typescript
   const createUser = () => { boundary.validate(...) };
   export { createUser };
   ```
   The traversal only inspects `ExportNamedDeclaration` containing a direct variable/function declaration, missing named export specifiers referring to local bindings.

3. **Classes / Methods:**
   `match.pattern: "UserService.createUser"` is completely unsupported, as `exported_function_span_from_statement` ignores `ClassDeclaration`.

## 3. The Integration Design
**Verdict: Clean and correctly placed.**
Injecting `check_envelope()` into `evaluate_contract_rules()` is the logical integration point. The refactored `check_match_patterns()` properly utilizes `globset` and feeds the `resolved_files` directly into the envelope check. This pipeline ensures we only parse what we absolutely must.

## 4. False Positive / Negative Analysis
- **False Positives (Tool complains, but code is safe):**
  - Wrapper HOCs (e.g., `export const handler = middleware(() => { ... })`).
  - Indirect exports (`export { handler }`).
  - Aliased default imports not matching `import_patterns`.
- **False Negatives (Tool passes, but code is unsafe):**
  - Reachability: `if (false) { boundary.validate(...) }` satisfies the AST check.
  - Shadowed/Reassigned variables: If a user locally shadows the `boundary` import to a dummy function, the AST check still counts it.
  - Dynamic contract IDs: `boundary.validate(myVar)` is ignored. If a user tries to dynamically generate contract IDs, Specgate won't detect the call, bypassing the static anchor requirement entirely.

## 5. Documentation Quality
**Verdict: Honest but incomplete.**
`docs/reference/envelope-guide.md` explicitly calls out the presence-based nature and lack of cross-file resolution, which is great expectation management. However, it *must* document the limitation around wrapper functions/HOCs and indirect exports, as those are incredibly common in Express/Next.js/tRPC routers. Adopters will immediately hit those walls and assume the feature is broken.

## 6. What Needs to Change for Production Readiness
Before calling this truly production-ready, the following adjustments are necessary:
1. **Fix or document the HOC/Wrapper limitation:** Either traverse `CallExpression` arguments looking for the inner function body in `find_exported_function_span()`, or explicitly state in `envelope-guide.md` that wrapped functions are not supported by `match.pattern`.
2. **Handle indirect exports:** At minimum, document that `export { fn }` is not supported for `match.pattern` scoping.
3. **Severity:** A contract marked `envelope: required` failing the check emits a `Severity::Warning` (hardcoded in `src/rules/contracts.rs`). If it's *required*, it should be an `Error` (or respect the global constraint severity, as boundary rules do). Hardcoding it to `Severity::Warning` dilutes the "required" semantic.
