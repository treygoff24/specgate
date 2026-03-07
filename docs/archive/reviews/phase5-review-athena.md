# Athena Review: Phase 5 Envelope AST Static Check

I've reviewed the implementation plan alongside the V2 spec and the codebase (`src/parser/mod.rs`, `src/rules/contracts.rs`, `src/spec/types.rs`, `src/spec/config.rs`). 

Overall, the deterministic AST-based approach is exactly what Paul's thesis calls for. However, the plan has a major architectural flaw regarding where the parsing happens, and a massive loophole regarding file-level granularity.

Here is my opinionated review.

## 1. Architecture Review: Separation of Concerns

**Status: WRONG**

The plan proposes injecting `envelope_imports` and `envelope_calls` into `FileAnalysis` via `src/parser/mod.rs`. This is the wrong abstraction for two reasons:
1. **Config Plumbing:** `parse_file(path: &Path)` currently has no access to `SpecConfig`. To match `envelope.function_pattern`, you would have to thread the project config through the entire generic file parsing and dependency graph building layer.
2. **Wasted CPU:** `specgate check` runs `parse_file` on *every single TS/JS file* in the dependency graph. Envelope checking only applies to a tiny fraction of files (the boundary handlers). You would be running string-matching for `boundary.validate` across 10,000 files just to verify 5 contracts.

**Concrete Alternative:**
Keep `src/parser/mod.rs` strictly about generic dependency edges (imports, exports, requires). Create a new module (e.g., `src/rules/envelope.rs` or inside `src/rules/contracts.rs`). Do a **second AST pass** *only* on the files resolved by `match.files` for contracts that actually have `envelope: required`. `oxc_parser` is blazingly fast; parsing 5 boundary files a second time will take <1ms. This keeps the generic graph builder clean and localizes contract logic.

## 2. The Fundamental Question: Granularity & False Positives

**Status: DANGEROUS LOOPHOLE**

The plan verifies that the *file* imports the envelope and contains the call. This creates a massive False Negative loophole. 

If a file `users.ts` exports two handlers (`createUser` and `deleteUser`), and only `createUser` calls `boundary.validate('create_user')`, the whole file passes the check! `deleteUser` can cross the boundary completely unvalidated, but Specgate will report 100% compliance because it found *a* call in the file.

**Concrete Alternative:**
The V2 spec introduced `match.pattern` (e.g., `pattern: "createUser"`) specifically for "AST-level binding". The Phase 5 plan completely ignores this! 
If `match.pattern` is present, the envelope check **must** verify that the `boundary.validate` call occurs *inside the AST node* of the matched function (`createUser`), not just anywhere in the file. If `pattern` is omitted, file-level is an acceptable (but porous) fallback.

## 3. Real-World TS/JS Patterns

The deterministic AST check will flag several real-world patterns as false positives (reporting missing envelopes when validation actually occurs):
- **Wrapper functions:** If `validate()` is in `utils.ts` and called in the handler, the AST check on the handler fails.
- **Middleware:** Express/Koa middleware runs in a separate file before the handler.
- **Decorators:** `@Validate` on a class method won't be caught by looking for `CallExpression`.
- **Dynamic IDs:** `boundary.validate(PREFIX + 'user', data)` will fail because the parser strictly looks for a `StringLiteral`.

**Opinion:** This is actually fine. Specgate is a *structural* enforcer. If teams want mechanical proof of validation, they must structure their code to make it statically provable (i.e., inline the wrapper, use string literals). However, the plan must **explicitly document these limitations** so users understand why their clever dynamic validation is failing the CI gate.

## 4. The "specgate-envelope" Dependency

**Status: CONTROVERSIAL BUT CORRECT**

Many teams use Zod, Joi, or custom validation directly (e.g., `userSchema.parse(data)`). The plan forces them to use an envelope wrapper that takes the contract ID as the first argument (`boundary.validate('create_user', userSchema, data)`).

Is this the right approach? **Yes.** 

Without the contract ID as a string literal, Specgate cannot deterministically link the validation call in the code to the specific contract ID declared in the YAML. If we just looked for `.parse()`, we'd have to do complex type-tracing to prove `userSchema` is the `create_user` contract. The string literal is the anchor that makes static AST analysis possible. 

**Concrete Alternative:** Defend this design decision aggressively in the docs. Explain *why* the wrapper is required (to anchor the static analysis) so users don't think we're just peddling another npm package.

## 5. Spec Alignment

The plan aligns well with `docs/specgate-boundary-contracts-v2.md`:
- Respects `envelope: required` vs `optional`.
- Makes the violation a `warning`.
- Honors the `--since` blast radius filtering naturally.

## 6. What's Missing Entirely?

1. **AST Parser Config Plumbing:** As mentioned in #1, the plan glosses over how `src/parser/mod.rs` gets the config. 
2. **Multiple Contracts in One File:** The plan's test cases don't cover a file that implements multiple contracts (e.g., `create_user` and `update_user`). The logic must ensure that *both* contract IDs are called if both are matched to the file.
3. **`match.pattern` Implementation:** The plan must be updated to include walking the AST to find the specific exported declaration matching `match.pattern`, and then scoping the `CallExpression` search to that specific sub-tree.

## Summary Verdict

Do not put envelope detection in `src/parser/mod.rs`. Move it to a targeted second pass in the rules engine. And absolutely implement `match.pattern` scoping to close the file-level false-negative loophole.