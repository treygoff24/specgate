# Phase 5 — Envelope AST Static Check: Code Review

**Reviewer:** opus-sub (Claude Opus 4.6)  
**Date:** 2026-03-08  
**Commits:** f613a81..de86b05 (6 commits)  
**Verdict:** ✅ Ship with noted improvements. No critical or high-severity bugs found.

---

## Summary

Phase 5 adds AST-level verification that files at boundary crossings import an
envelope validation package and call the validator with the correct contract ID.
The implementation spans 6 files: a new `EnvelopeConfig` struct in config, a new
1181-line AST analyzer module (`envelope.rs`), integration logic in
`contracts.rs`, a severity wiring fix in `cli/mod.rs`, 10 integration tests, and
9 fixture scenarios.

Overall quality is high. The AST visitor is thorough and handles the
TypeScript-specific AST wrappers (`TSAsExpression`, `TSNonNullExpression`, etc.)
that trip up most static analyzers. The test coverage is meaningful and the
fixture-based integration tests exercise the full pipeline. The severity wiring
fix for `ContractRuleViolation` was an important pre-existing bug caught and
fixed in this phase.

---

## Findings

### 🔵 Medium

#### M1. Namespace imports silently ignored — potential false negative

**File:** `src/rules/envelope.rs`, line 275  
**Pattern:** `import * as env from 'specgate-envelope'`

```rust
oxc_ast::ast::ImportDeclarationSpecifier::ImportNamespaceSpecifier(_) => {
    // Out of scope for envelope binding matching.
}
```

When a user writes `import * as env from 'specgate-envelope'; env.boundary.validate('id', data)`,
the import is detected as a matching import (the package name matches), but no
binding is recorded (`has_runtime_binding` stays false → `has_envelope_import`
stays false). The call `env.boundary.validate(...)` will also fail
`matches_call_pattern` because the pattern only supports 2-part dot notation
(`object.method`), not 3-part (`namespace.object.method`).

Result: **false negative** — the user has a valid envelope call but gets a
violation reported.

The comment "Out of scope" is fine as a conscious design choice, but should be
documented in the spec config reference or emit a warning so users aren't
confused. The existing test for `require()` shows that bare `require()` sets
`has_envelope_import` without creating bindings and then the `boundary.validate`
call works via the fallback path in `matches_call_pattern` (line 773:
`state.has_envelope_import && object_identifier == object_name`). A similar
fallback for namespace imports would close this gap.

**Recommendation:** Either (a) record the namespace local name as a binding and
handle 3-part patterns, or (b) at minimum register `has_envelope_import = true`
for namespace imports (like `require()` does) so the "missing import" false
positive is avoided — the "missing call" will still fire but that's a more
accurate diagnostic.

---

#### M2. `check_envelope` re-reads and re-parses files unnecessarily

**File:** `src/rules/contracts.rs`, lines 304–360  
**File:** `src/rules/envelope.rs`, lines 97–107

When `match.pattern` is specified, `check_envelope` calls
`analyze_file_for_envelope` (which reads + parses the file), then reads the file
*again* with `fs::read_to_string` and calls `find_exported_function_span` (which
parses it a *second* time). That's 2 reads + 2 parses of the same file.

The first parse (`analyze_file_for_envelope`) already has the full AST. The
span-finding logic could be folded into the analyzer or the source text could be
returned from `analyze_file_for_envelope` to avoid the second read.

**Impact:** Correctness is unaffected. Performance penalty is proportional to
the number of files with `match.pattern` contracts. For most projects this is
negligible, but on large monorepos with many boundary contracts it could be
measurable.

**Recommendation:** Consider returning the source text from
`analyze_file_for_envelope` (or accepting `&str` + `&Path` instead of just
`&Path`) so `check_envelope` can reuse it for `find_exported_function_span`.

---

#### M3. `has_envelope_import || is_type_only_import` condition is subtly wrong

**File:** `src/rules/contracts.rs`, line 311

```rust
if !analysis.has_envelope_import || analysis.is_type_only_import {
```

Consider a file with *both* a type-only import and a runtime import from the
envelope package:

```typescript
import type { EnvelopeConfig } from 'specgate-envelope';
import { boundary } from 'specgate-envelope';
boundary.validate('create_user', data);
```

In this case, `has_envelope_import` is `true` AND `is_type_only_import` is
`true` (the first import sets the flag). The condition
`!true || true → false || true → true` means this file would be flagged as
missing an import even though the runtime import is present.

**Root cause:** `is_type_only_import` is set as a sticky flag in
`collect_import_info` (line 255) and is never cleared when a subsequent runtime
import is found.

**Impact:** False positive when a file has both type-only and runtime imports
from the envelope package. This is an uncommon but valid pattern (e.g., importing
types for annotations alongside runtime validators).

**Recommendation:** Change the condition to:
```rust
if !analysis.has_envelope_import {
    // report missing import (type-only flag is informational)
```
The `is_type_only_import` flag is only needed when `has_envelope_import` is false
to provide a more specific diagnostic message. The current condition incorrectly
treats it as a disqualifier.

---

### 🟢 Low

#### L1. `Err(_) => continue` silently swallows envelope analysis errors

**File:** `src/rules/contracts.rs`, line 308

```rust
let analysis = match envelope::analyze_file_for_envelope(...) {
    Ok(analysis) => analysis,
    Err(_) => continue,
};
```

If a matched file can't be read or parsed, the error is silently swallowed and
the file is skipped. No diagnostic is emitted. A user could have a
misconfigured project where envelope files fail to parse and would never know
the check was skipped.

**Recommendation:** At minimum, log or collect a warning-level diagnostic
indicating the file was skipped due to a parse/read error.

---

#### L2. `find_exported_function_span` doesn't handle `export const x = function name()` correctly

**File:** `src/rules/envelope.rs`, lines 148–162

The code handles `export const createUser = function(...) { ... }` but the
function expression body span extraction calls
`function_expression_body_span(function.body.as_ref())` where the signature is:

```rust
fn function_expression_body_span(
    body: Option<&oxc_allocator::Box<'_, oxc_ast::ast::FunctionBody<'_>>>,
) -> Option<(u32, u32)> {
    body.map(|body| (body.span.start, body.span.end))
}
```

This is correct — the `body` field of `FunctionExpression` is
`Option<Box<FunctionBody>>`. However, oxc's `FunctionExpression` always has a
body (it's not optional for function expressions, only for type declarations).
The `Option` wrapper is an artifact of the shared `Function` type.

No bug here, but the indirection through a standalone function for a one-liner
is unusual. Could be inlined.

---

#### L3. `expression_contract_id` only handles string literals and static template literals

**File:** `src/rules/envelope.rs`, lines 803–816

Dynamic template literals (`` `create_${suffix}` ``) and const enum members used
as contract IDs will not be extracted. The current behavior (returning `None` →
call is ignored) is conservative and correct — the analyzer won't produce false
positives from dynamic contract IDs.

However, this means a `boundary.validate(ContractId.CREATE_USER, data)` pattern
using const enums is a **false negative** — the call is present but unrecognized.

**Recommendation:** Document this limitation. Users should be told to use string
literal contract IDs.

---

#### L4. `filter_calls_by_span` uses inclusive-start/inclusive-end comparison

**File:** `src/rules/envelope.rs`, lines 186–193

```rust
calls.iter()
    .filter(|call| call.span_start >= span_start && call.span_end <= span_end)
```

This works correctly for the intended use case (function body spans include
their opening `{` and closing `}`), but the boundary condition could be subtle:
a call that starts exactly at `span_start` or ends exactly at `span_end` is
included. Given that `span_start` is the `{` of the body and calls are always
inside the braces, this is correct.

No action needed — noting for documentation purposes.

---

### ⚪ Nitpick

#### N1. Redundant guard in original `check_match_patterns`

**File:** `src/rules/contracts.rs`, line 258 (after refactor)

The `if !any_resolved` guard was previously `if !any_resolved && !contract.r#match.files.is_empty()`,
but the early return at line 219 for `files.is_empty()` already guarantees files
is non-empty at this point. The refactored version correctly simplified to just
`if !any_resolved`. Good cleanup.

---

#### N2. Sorting calls and import_bindings

**File:** `src/rules/envelope.rs`, lines 68–77

The calls are sorted by `(line, column, contract_id)` for deterministic output.
Import bindings use a `BTreeSet` which is inherently sorted. Both are good
practices for deterministic test assertions and output stability.

---

#### N3. `ContractRuleViolation::new` takes `impl Into<String>` for some params but not all

**File:** `src/rules/contracts.rs`, lines 48–50

`remediation_hint` and `contract_id` use `impl Into<String>` for ergonomic
construction, but `violation` and `severity` do not (they're concrete types).
This is fine and consistent with Rust conventions — `Into<String>` is only useful
for string-like parameters.

---

#### N4. Test helper `block_span_from_source` uses `find('{')/rfind('}')`

**File:** `src/rules/envelope.rs`, lines ~928–933

This works for the simple test sources used but would break for sources with
multiple top-level braces. Since it's test-only code and the test sources are
deliberately simple, this is fine.

---

## Integration Risk Assessment

### Severity field addition to `ContractRuleViolation`

The addition of `severity: Severity` to `ContractRuleViolation` is a breaking
change to the struct's constructor (callers must now pass severity). All internal
callers were updated. The `cli/mod.rs` fix from `Severity::Error` to
`contract_violation.severity` is the critical correctness fix — without it,
envelope violations would be reported as errors instead of warnings, changing
exit codes and breaking CI pipelines.

**Risk:** Low. The change is internal-only (no public API surface beyond the
CLI). All existing contract rule violations (missing file, empty file, unresolved
match, invalid ref) are hardcoded to `Severity::Error` as before. Only the new
envelope violations use `Severity::Warning`.

### `check_match_patterns` refactor

The return type changed from `Option<ContractRuleViolation>` to
`(Option<ContractRuleViolation>, Vec<PathBuf>)`. The refactored code preserves
the exact same violation logic and adds file collection as a second return value.
The sorting and dedup of `resolved_files` prevents duplicate envelope checks on
the same file if multiple glob patterns match it.

**Risk:** Very low. The violation logic path is unchanged; only the file
collection path is new. The existing tests cover the violation path.

### Double-parse performance

The envelope check does 2 full AST parses per matched file when `match.pattern`
is specified (and 1 parse otherwise). For most projects, the number of
boundary-crossing files is small (<100), and oxc parsing is fast (~1ms per file).
The performance impact is negligible for the target use case.

---

## Test Coverage Assessment

### Strengths

- 9 fixture scenarios cover the major happy/unhappy paths: valid basic, missing
  import, missing call, wrong ID, optional skip, disabled config, match pattern
  scoped, match pattern wrong function, type-only import.
- Integration tests use the full CLI pipeline (`run` function), ensuring the
  verdict JSON structure is correct end-to-end.
- Unit tests in `envelope.rs` are extensive (25 tests) covering ESM imports,
  default imports, renamed imports, require(), template literals, TS assertions,
  optional chaining, multiple calls, custom import patterns, and span filtering.
- Unit tests in `contracts.rs` cover the integration of envelope checks with the
  contract rule engine, including scoping, severity, and config-level disabling.

### Gaps

1. **No test for namespace imports** (`import * as env from ...`). Given M1
   above, this gap is expected but should be filled.
2. **No test for multiple imports from the same package** (runtime + type-only).
   This would catch M3.
3. **No test for deeply nested call patterns** (calls inside callbacks, promises,
   `then` chains, `try/catch`). The visitor handles these paths, but there's no
   test validating that a `boundary.validate(...)` inside a
   `somePromise.then(() => { boundary.validate('id', data); })` is detected.
4. **No test for `.js`/`.jsx`/`.mjs` file extensions.** The analyzer uses
   `SourceType::from_path` which should handle these, but no fixture verifies it.
5. **No negative test for `check_envelope` error path** — what happens when a
   matched file is unreadable (permissions, deleted between glob scan and read)?
6. **No test for CommonJS `require()` with destructuring:**
   `const { boundary } = require('specgate-envelope')`. The current code only
   detects bare `require()` calls via `common_js_require()` and sets
   `has_envelope_import = true` without extracting bindings. The call detection
   then works via the fallback `has_envelope_import && object_identifier == object_name`
   in `matches_call_pattern`. But if the user destructures differently
   (e.g., `const { validate } = require('specgate-envelope')`), the single-part
   pattern `validate` won't match because `is_import_binding("validate")` is false
   and there's no binding registered for require-based imports.

---

## Architecture Notes

The design cleanly separates concerns:

1. **`envelope.rs`** — pure AST analysis. Takes a file path + patterns, returns
   a structural analysis result. No policy decisions.
2. **`contracts.rs`** — policy integration. Decides *when* to run the analysis
   (envelope required? enabled? files resolved?), interprets results against
   contract specs, and produces policy violations.
3. **`config.rs`** — configuration surface. Clean defaults, serde integration,
   validation tests.
4. **`cli/mod.rs`** — wiring. Maps `ContractRuleViolation.severity` into
   `PolicyViolation.severity`.

The module boundaries are well-chosen. The `EnvelopeAnalysis` struct is a good
intermediate representation that decouples analysis from policy enforcement.

---

## Final Assessment

| Category | Rating |
|----------|--------|
| Correctness | Good — one edge case bug (M3) with dual imports |
| Test coverage | Good — covers major paths, has identified gaps |
| Code quality | High — clean abstractions, thorough visitor |
| Performance | Acceptable — double parse is noted but not blocking |
| Integration safety | High — severity fix was the right call |
| API design | Clean — good separation of analysis vs policy |

**Recommendation:** Ship as-is. Fix M3 (dual import false positive) in a fast
follow since it's a logic error in a condition. M1 and M2 can go in the backlog.
